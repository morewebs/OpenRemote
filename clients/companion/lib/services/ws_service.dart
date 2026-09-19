import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:web_socket_channel/web_socket_channel.dart';
import 'package:http/http.dart' as http;

/// dart2js does not implement ByteData.setUint64. Encode the wire timestamp
/// using two 32-bit words so web and native clients share the same framing.
Uint8List encodePingFrame(int timestamp) {
  final frame = Uint8List(10);
  frame[0] = 0x06;
  final data = ByteData.sublistView(frame, 2);
  data.setUint32(0, timestamp ~/ 0x100000000, Endian.big);
  data.setUint32(4, timestamp % 0x100000000, Endian.big);
  return frame;
}

class WebSocketService {
  WebSocketChannel? _channel;
  StreamSubscription? _sub;
  Timer? _pingTimer;
  Timer? _reconnectTimer;
  http.Client? _sseClient;
  bool _usingSse = false;
  Future<void> _inputQueue = Future.value();

  final void Function(Uint8List bytes)? onPtyOutput;
  final void Function(Map<String, dynamic> event)? onJsonRpc;
  final void Function(bool connected)? onConnectionChange;

  bool _isConnected = false;
  bool _userDisconnected = false;
  int _generation = 0;
  int _reconnectAttempts = 0;
  int _lastSeq = 0;

  String _wsUrl = '';
  String? _sessionId;
  String? _token;

  bool get isConnected => _isConnected;
  int get lastSeq => _lastSeq;

  WebSocketService({this.onPtyOutput, this.onJsonRpc, this.onConnectionChange});

  void connect(String wsUrl, {String? sessionId, String? token}) {
    disconnect();
    _lastSeq = 0;
    _reconnectAttempts = 0;
    _usingSse = false;
    _userDisconnected = false;
    _wsUrl = wsUrl;
    _sessionId = sessionId;
    _token = token;
    _openChannel();
  }

  Future<void> _openChannel() async {
    if (_userDisconnected) return;
    _reconnectTimer?.cancel();
    _reconnectTimer = null;
    _teardownChannel();
    final generation = ++_generation;

    var uri = Uri.parse(_wsUrl);
    final queryParams = Map<String, String>.from(uri.queryParameters);
    queryParams['eventsOnly'] = '1';
    queryParams['lastSeq'] = '$_lastSeq';
    if (_sessionId != null && _sessionId!.isNotEmpty) {
      queryParams['sessionId'] = _sessionId!;
    }
    if (_token != null && _token!.isNotEmpty) {
      queryParams['token'] = _token!;
    }
    uri = uri.replace(queryParameters: queryParams);

    try {
      if (_reconnectAttempts >= 3 || _usingSse) {
        await _openSse(uri, generation);
        return;
      }
      final channel = WebSocketChannel.connect(uri);
      _channel = channel;

      _sub = channel.stream.listen(
        (data) {
          if (generation == _generation) _handleIncoming(data);
        },
        onError: (err) {
          if (generation == _generation) _handleDisconnect();
        },
        onDone: () {
          if (generation == _generation) _handleDisconnect();
        },
      );

      await channel.ready;
      if (generation != _generation || _userDisconnected) return;
      _isConnected = true;
      _reconnectAttempts = 0;
      onConnectionChange?.call(true);
      _startPing();
    } catch (e) {
      if (generation == _generation) _handleDisconnect();
    }
  }

  Future<void> _openSse(Uri wsUri, int generation) async {
    final client = http.Client();
    _sseClient = client;
    final uri = wsUri.replace(
      scheme: wsUri.scheme == 'wss' ? 'https' : 'http',
      path: wsUri.path.replaceFirst(RegExp(r'/ws$'), '/events'),
    );
    final response = await client.send(http.Request('GET', uri));
    if (generation != _generation || _userDisconnected) {
      client.close();
      return;
    }
    if (response.statusCode != 200) {
      throw StateError('Event stream failed: ${response.statusCode}');
    }
    _usingSse = true;
    _isConnected = true;
    onConnectionChange?.call(true);
    final lines = <String>[];
    _sub = response.stream
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(
          (line) {
            if (generation != _generation) return;
            if (line.isEmpty) {
              if (lines.isNotEmpty) _decodeAndDispatchString(lines.join('\n'));
              lines.clear();
            } else if (line.startsWith('data:')) {
              lines.add(line.substring(5).trimLeft());
            }
          },
          onError: (Object error) {
            if (generation == _generation) _handleDisconnect();
          },
          onDone: () {
            if (generation == _generation) _handleDisconnect();
          },
        );
  }

  void _postTerminal(String action, Map<String, dynamic> body) {
    final generation = _generation;
    _inputQueue = _inputQueue
        .then((_) async {
          if (generation != _generation || !_isConnected) return;
          final wsUri = Uri.parse(_wsUrl);
          final uri = wsUri.replace(
            scheme: wsUri.scheme == 'wss' ? 'https' : 'http',
            path:
                '/api/v1/sessions/${Uri.encodeComponent(_sessionId ?? '')}/$action',
            query: '',
          );
          final response = await _sseClient!
              .post(
                uri,
                headers: {
                  'Content-Type': 'application/json',
                  if (_token?.isNotEmpty == true)
                    'Authorization': 'Bearer $_token',
                },
                body: jsonEncode(body),
              )
              .timeout(const Duration(seconds: 10));
          if (response.statusCode != 200) {
            throw StateError('Terminal input failed');
          }
        })
        .catchError((Object error) {
          if (generation == _generation) _handleDisconnect();
        });
  }

  void _handleDisconnect() {
    if (!_isConnected && _reconnectTimer != null) return;
    _generation++;
    _teardownChannel();
    onConnectionChange?.call(false);

    if (_userDisconnected) return;

    // Exponential backoff: 1s, 2s, 4s, ... capped at 15s.
    final delay = Duration(
      seconds: _reconnectAttempts < 4 ? (1 << _reconnectAttempts) : 15,
    );
    _reconnectAttempts++;
    _reconnectTimer = Timer(delay, _openChannel);
  }

  void _teardownChannel() {
    _sseClient?.close();
    _sseClient = null;
    _pingTimer?.cancel();
    _pingTimer = null;
    _sub?.cancel();
    _sub = null;
    try {
      _channel?.sink.close();
    } catch (_) {}
    _channel = null;
    _isConnected = false;
  }

  void _handleIncoming(dynamic data) {
    if (data is List<int>) {
      if (data.length < 2) return;
      final opcode = data[0];
      final payload = Uint8List.fromList(data.sublist(2));

      switch (opcode) {
        case 0x01: // PTY Output
          onPtyOutput?.call(payload);
          break;
        case 0x05: // JSON-RPC / Event
          _decodeAndDispatch(payload);
          break;
      }
    } else if (data is String) {
      _decodeAndDispatchString(data);
    }
  }

  void _decodeAndDispatch(Uint8List payload) {
    try {
      _decodeAndDispatchString(utf8.decode(payload));
    } catch (_) {}
  }

  void _decodeAndDispatchString(String jsonStr) {
    try {
      final map = jsonDecode(jsonStr);
      if (map is Map<String, dynamic>) {
        final seq = map['seq'];
        if (seq is num && seq > 0) {
          if (seq <= _lastSeq) return;
          _lastSeq = seq.toInt();
        }
        if (map['type'] == 'stream.chunk') {
          final chunk = map['chunk'] as String? ?? '';
          onPtyOutput?.call(
            map['encoding'] == 'base64'
                ? base64.decode(chunk)
                : Uint8List.fromList(utf8.encode(chunk)),
          );
        }
        onJsonRpc?.call(map);
      }
    } catch (_) {}
  }

  void sendKeystroke(Uint8List bytes, {int slot = 0}) {
    if (_usingSse && _isConnected) {
      _postTerminal('input', {'data': base64.encode(bytes)});
      return;
    }
    if (!_isConnected || _channel == null) return;
    final frame = Uint8List(2 + bytes.length);
    frame[0] = 0x02; // OpcodeKeystroke
    frame[1] = slot;
    frame.setRange(2, frame.length, bytes);
    _channel!.sink.add(frame);
  }

  void sendResize(int cols, int rows, {int slot = 0}) {
    cols = cols.clamp(20, 300);
    rows = rows.clamp(5, 100);
    if (_usingSse && _isConnected) {
      _postTerminal('resize', {'cols': cols, 'rows': rows});
      return;
    }
    if (!_isConnected || _channel == null) return;
    final frame = Uint8List(6);
    frame[0] = 0x03; // OpcodeViewportResize
    frame[1] = slot;
    final bd = ByteData.sublistView(frame, 2);
    bd.setUint16(0, cols, Endian.big);
    bd.setUint16(2, rows, Endian.big);
    _channel!.sink.add(frame);
  }

  void sendCatchup(int lastSeq, {int slot = 0}) {
    if (!_isConnected || _channel == null) return;
    final frame = Uint8List(6);
    frame[0] = 0x04; // OpcodeCatchup
    frame[1] = slot;
    final bd = ByteData.sublistView(frame, 2);
    bd.setUint32(0, lastSeq, Endian.big);
    _channel!.sink.add(frame);
  }

  void _startPing() {
    _pingTimer?.cancel();
    _pingTimer = Timer.periodic(const Duration(seconds: 15), (t) {
      if (!_isConnected || _channel == null) {
        t.cancel();
        return;
      }
      _channel!.sink.add(
        encodePingFrame(DateTime.now().millisecondsSinceEpoch),
      );
    });
  }

  void disconnect() {
    _generation++;
    _userDisconnected = true;
    _reconnectTimer?.cancel();
    _reconnectTimer = null;
    _teardownChannel();
    onConnectionChange?.call(false);
  }
}
