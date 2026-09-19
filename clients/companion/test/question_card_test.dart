import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:companion/features/chat/widgets/question_card.dart';
import 'package:companion/models/models.dart';

void main() {
  testWidgets(
    'multi-select sends all selections once while delivery is pending',
    (tester) async {
      final delivery = Completer<void>();
      final sent = <List<dynamic>>[];
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: QuestionCard(
              question: PendingQuestion.fromJson({
                'questionId': 'q1',
                'sessionId': 's1',
                'questionText': 'Pick colors',
                'options': ['Red', 'Blue'],
                'isMultiSelect': true,
              }),
              onAnswer: (answers) {
                sent.add(answers);
                return delivery.future;
              },
            ),
          ),
        ),
      );
      await tester.tap(find.text('Red'));
      await tester.pump();
      await tester.tap(find.text('Blue'));
      await tester.pump();
      await tester.tap(find.text('Answer'));
      await tester.pump();
      expect(sent, [
        ['Red', 'Blue'],
      ]);
      final button = tester.widget<ElevatedButton>(find.byType(ElevatedButton));
      expect(button.onPressed, isNull);
      delivery.complete();
      await tester.pump();
      expect(find.text('Answer'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );
}
