import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:companion/services/ws_service.dart';

void main() {
  test('reconnect resumes the cursor and deduplicates binary replay', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    final sockets = <WebSocket>[];
    final received = <int>[];
    final bytes = <int>[];
    final finished = Completer<void>();
    var connections = 0;
    final subscription = server.listen((request) async {
      connections++;
      expect(
        request.uri.queryParameters['lastSeq'],
        connections == 1 ? '0' : '1',
      );
      final socket = await WebSocketTransformer.upgrade(request);
      sockets.add(socket);
      void send(int seq, List<int> chunk) {
        socket.add([
          5,
          0,
          ...utf8.encode(
            jsonEncode({
              'seq': seq,
              'type': 'stream.chunk',
              'encoding': 'base64',
              'chunk': base64Encode(chunk),
            }),
          ),
        ]);
      }

      final raw = utf8.encode('🌍');
      send(1, raw.sublist(0, 2));
      if (connections == 1) {
        await socket.close();
      } else {
        send(2, raw.sublist(2));
      }
    });
    final client = WebSocketService(
      onPtyOutput: bytes.addAll,
      onJsonRpc: (event) {
        received.add(event['seq'] as int);
        if (event['seq'] == 2 && !finished.isCompleted) finished.complete();
      },
    );
    try {
      client.connect('ws://127.0.0.1:${server.port}/ws', sessionId: 's1');
      await finished.future.timeout(const Duration(seconds: 8));
      expect(received, [1, 2]);
      expect(utf8.decode(bytes), '🌍');
    } finally {
      client.disconnect();
      for (final socket in sockets) {
        await socket.close();
      }
      await subscription.cancel();
      await server.close(force: true);
    }
  });
}
