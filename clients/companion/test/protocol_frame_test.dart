import 'package:flutter_test/flutter_test.dart';
import 'package:companion/services/ws_service.dart';

void main() {
  test('ping timestamp encodes both words in network byte order', () {
    expect(encodePingFrame(0x0123456789ab), [
      6,
      0,
      0,
      0,
      1,
      0x23,
      0x45,
      0x67,
      0x89,
      0xab,
    ]);
    expect(encodePingFrame(0), [6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
  });
}
