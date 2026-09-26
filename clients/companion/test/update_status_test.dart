import 'package:flutter_test/flutter_test.dart';
import 'package:companion/models/models.dart';

void main() {
  test('parses an available update with full fields', () {
    final status = DaemonUpdateStatus.fromJson({
      'current': '0.9.0',
      'latest': '0.10.0',
      'available': true,
      'checkedAt': '2026-09-26T17:00:00Z',
      'autoCheck': '24h0m0s',
      'applying': false,
      'applyError': '',
    });
    expect(status.current, '0.9.0');
    expect(status.latest, '0.10.0');
    expect(status.available, isTrue);
    expect(status.autoCheck, '24h0m0s');
    expect(status.applying, isFalse);
    expect(status.applyError, '');
  });

  test('parses a failed apply and defaults missing fields', () {
    final status = DaemonUpdateStatus.fromJson({
      'current': '0.9.0',
      'available': false,
      'applying': true,
      'applyError': 'checksum mismatch: refusing to apply',
    });
    expect(status.latest, isNull);
    expect(status.available, isFalse);
    expect(status.applying, isTrue);
    expect(status.applyError, contains('checksum mismatch'));
    expect(status.autoCheck, 'off');
    expect(status.checkedAt, isNull);
  });
}
