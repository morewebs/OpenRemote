import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:web_socket_channel/web_socket_channel.dart';

class WebSocketService {
  WebSocketChannel? _channel;
  StreamSubscription? _sub;
  Timer? _pingTimer;
  Timer? _reconnectTimer;

  final void Function(Uint8List bytes)? onPtyOutput;
  final void Function(Map<String, dynamic> event)? onJsonRpc;
  final void Function(bool connected)? onConnectionChange;

  bool _isConnected = false;
  bool _userDisconnected = false;
  bool _hadFirstConnect = false;
  int _reconnectAttempts = 0;
  int _lastSeq = 0;

  String _wsUrl = '';
  String? _sessionId;
  String? _token;

  bool get isConnected => _isConnected;
  int get lastSeq => _lastSeq;

  WebSocketService({
    this.onPtyOutput,
    this.onJsonRpc,
    this.onConnectionChange,
  });

  void connect(String wsUrl, {String? sessionId, String? token}) {
    _userDisconnected = false;
    _wsUrl = wsUrl;
    _sessionId = sessionId;
    _token = token;
    _openChannel();
  }

  void _openChannel() {
    _teardownChannel();

    var uri = Uri.parse(_wsUrl);
    final queryParams = Map<String, String>.from(uri.queryParameters);
    if (_sessionId != null && _sessionId!.isNotEmpty) {
      queryParams['sessionId'] = _sessionId!;
    }
    if (_token != null && _token!.isNotEmpty) {
      queryParams['token'] = _token!;
    }
    uri = uri.replace(queryParameters: queryParams);

    try {
      _channel = WebSocketChannel.connect(uri);
      _isConnected = true;
      _reconnectAttempts = 0;
      onConnectionChange?.call(true);

      _sub = _channel!.stream.listen(
        (data) {
          _handleIncoming(data);
        },
        onError: (err) {
          _handleDisconnect();
        },
        onDone: () {
          _handleDisconnect();
        },
      );

      _startPing();

      if (_hadFirstConnect) {
        // Reconnected after a drop: replay events missed while offline.
        sendCatchup(_lastSeq);
      }
      _hadFirstConnect = true;
    } catch (e) {
      _handleDisconnect();
    }
  }

  void _handleDisconnect() {
    if (!_isConnected && _reconnectTimer != null) return;
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
    if (data is Uint8List) {
      if (data.length < 2) return;
      final opcode = data[0];
      final payload = data.sublist(2);

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
        if (seq is num && seq > _lastSeq) {
          _lastSeq = seq.toInt();
        }
        onJsonRpc?.call(map);
      }
    } catch (_) {}
  }

  void sendKeystroke(Uint8List bytes, {int slot = 0}) {
    if (!_isConnected || _channel == null) return;
    final frame = Uint8List(2 + bytes.length);
    frame[0] = 0x02; // OpcodeKeystroke
    frame[1] = slot;
    frame.setRange(2, frame.length, bytes);
    _channel!.sink.add(frame);
  }

  void sendResize(int cols, int rows, {int slot = 0}) {
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
      final frame = Uint8List(10);
      frame[0] = 0x06; // OpcodePingPong
      frame[1] = 0;
      final bd = ByteData.sublistView(frame, 2);
      bd.setUint64(0, DateTime.now().millisecondsSinceEpoch, Endian.big);
      _channel!.sink.add(frame);
    });
  }

  void disconnect() {
    _userDisconnected = true;
    _reconnectTimer?.cancel();
    _reconnectTimer = null;
    _teardownChannel();
    onConnectionChange?.call(false);
  }
}
