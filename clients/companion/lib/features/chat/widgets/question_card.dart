import 'package:flutter/material.dart';
import '../../../models/models.dart';
import '../../../theme/theme.dart';

/// Interactive disambiguation question card: single-select radio list plus a
/// write-in field, mirroring the agent's in-terminal option dialog.
class QuestionCard extends StatefulWidget {
  final PendingQuestion question;
  final Future<void> Function(List<dynamic> answers) onAnswer;

  const QuestionCard({
    super.key,
    required this.question,
    required this.onAnswer,
  });

  @override
  State<QuestionCard> createState() => QuestionCardState();
}

class QuestionCardState extends State<QuestionCard> {
  int? _selected;
  final Set<int> _selections = {};
  bool _submitting = false;
  final TextEditingController _customController = TextEditingController();
  bool _useCustom = false;

  @override
  void dispose() {
    _customController.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_submitting) return;
    final answers = _useCustom
        ? [_customController.text.trim()]
        : widget.question.isMultiSelect
        ? (_selections.toList()..sort())
              .map((i) => widget.question.options[i])
              .toList()
        : [if (_selected != null) widget.question.options[_selected!]];
    if (answers.isEmpty || answers.any((a) => a.isEmpty)) return;
    setState(() => _submitting = true);
    try {
      await widget.onAnswer(answers);
    } finally {
      if (mounted) setState(() => _submitting = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final q = widget.question;
    final canSubmit = _useCustom
        ? _customController.text.trim().isNotEmpty
        : q.isMultiSelect
        ? _selections.isNotEmpty
        : _selected != null;

    return Container(
      margin: const EdgeInsets.all(12),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: AppTheme.surfaceDark,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTheme.purpleAccent, width: 1.5),
        boxShadow: const [
          BoxShadow(
            color: AppTheme.purpleGlow,
            blurRadius: 12,
            spreadRadius: 1,
          ),
        ],
      ),
      child: Material(
        color: Colors.transparent,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Icon(
                  Icons.help_outline,
                  color: AppTheme.purpleLight,
                  size: 18,
                ),
                const SizedBox(width: 8),
                const Text(
                  'Question',
                  style: TextStyle(
                    fontWeight: FontWeight.w700,
                    fontSize: 14,
                    color: AppTheme.textMain,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text(
              q.questionText,
              style: const TextStyle(
                color: AppTheme.textMain,
                fontSize: 13.5,
                height: 1.4,
              ),
            ),
            const SizedBox(height: 10),
            if (q.isMultiSelect)
              ...List.generate(
                q.options.length,
                (idx) => CheckboxListTile(
                  dense: true,
                  contentPadding: EdgeInsets.zero,
                  controlAffinity: ListTileControlAffinity.leading,
                  title: Text(q.options[idx]),
                  value: !_useCustom && _selections.contains(idx),
                  onChanged: _submitting
                      ? null
                      : (selected) => setState(() {
                          _useCustom = false;
                          if (selected == true) {
                            _selections.add(idx);
                          } else {
                            _selections.remove(idx);
                          }
                        }),
                ),
              ),
            AbsorbPointer(
              absorbing: _submitting,
              child: RadioGroup<int>(
                groupValue: _useCustom ? -1 : (_selected ?? -2),
                onChanged: (v) => setState(() {
                  if (v != null && v >= 0 && v < q.options.length) {
                    _selected = v;
                    _useCustom = false;
                  } else {
                    _useCustom = true;
                    _selected = null;
                  }
                }),
                child: Column(
                  children: [
                    if (!q.isMultiSelect)
                      ...List.generate(q.options.length, (idx) {
                        return RadioListTile<int>(
                          dense: true,
                          contentPadding: EdgeInsets.zero,
                          visualDensity: VisualDensity.compact,
                          value: idx,
                          title: Text(
                            q.options[idx],
                            style: const TextStyle(
                              color: AppTheme.textMain,
                              fontSize: 13,
                            ),
                          ),
                        );
                      }),
                    InkWell(
                      onTap: () => setState(() {
                        _useCustom = true;
                        _selected = null;
                      }),
                      child: Padding(
                        padding: const EdgeInsets.symmetric(vertical: 6),
                        child: Row(
                          children: [
                            Icon(
                              _useCustom
                                  ? Icons.radio_button_checked
                                  : Icons.radio_button_off,
                              size: 20,
                              color: _useCustom
                                  ? AppTheme.purpleAccent
                                  : AppTheme.textMuted,
                            ),
                            const SizedBox(width: 12),
                            Expanded(
                              child: TextField(
                                controller: _customController,
                                style: const TextStyle(
                                  color: AppTheme.textMain,
                                  fontSize: 13,
                                ),
                                decoration: const InputDecoration(
                                  hintText: 'Write your own answer...',
                                  isDense: true,
                                  border: InputBorder.none,
                                  contentPadding: EdgeInsets.symmetric(
                                    vertical: 8,
                                  ),
                                ),
                                onTap: () => setState(() {
                                  _useCustom = true;
                                  _selected = null;
                                }),
                                onChanged: (_) => setState(() {}),
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 10),
            Align(
              alignment: Alignment.centerRight,
              child: ElevatedButton.icon(
                icon: const Icon(Icons.send, size: 15),
                label: Text(_submitting ? 'Sending…' : 'Answer'),
                onPressed: canSubmit && !_submitting ? _submit : null,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
