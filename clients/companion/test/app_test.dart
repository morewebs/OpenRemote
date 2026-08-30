import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:companion/app.dart';

void main() {
  testWidgets('App renders OpenRemote title', (WidgetTester tester) async {
    await tester.pumpWidget(
      const ProviderScope(
        child: OpenRemoteApp(),
      ),
    );

    // Initial frame
    await tester.pump();

    expect(find.text('OpenRemote'), findsWidgets);
  });
}
