import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown/flutter_markdown.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:google_fonts/google_fonts.dart';
import '../../models/models.dart';
import '../../providers/providers.dart';
import '../../services/ws_service.dart';
import '../../theme/theme.dart';

class ChatScreen extends ConsumerStatefulWidget {
  final String sessionId;
  const ChatScreen({super.key, required this.sessionId});

  @override
  ConsumerState<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends ConsumerState<ChatScreen> {
  final TextEditingController _textController = TextEditingController();
  final ScrollController _scrollController = ScrollController();
  WebSocketService? _ws;

  @override
  void initState() {
    super.initState();
    Future.microtask(() {
      ref.read(activeSessionIdProvider.notifier).state = widget.sessionId;
      _hydrateTranscript();
      _initWebSocket();
    });
  }

  /// Replays persisted session events so a freshly opened chat shows the
  /// full transcript, pending approvals, and questions.
  Future<void> _hydrateTranscript() async {
    try {
      final events = await ref.read(apiServiceProvider).getEvents(widget.sessionId);
      if (!mounted) return;
      for (final ev in events) {
        _dispatchEvent(ev);
      }
      _scrollToBottom();
    } catch (_) {
      // Hydration is best-effort; live events still arrive via WebSocket.
    }
  }

  void _initWebSocket() {
    final config = ref.read(serverConfigProvider);
    final wsBase = config.baseUrl.replaceFirst('http', 'ws');
    final wsUrl = '$wsBase/ws';

    _ws = WebSocketService(
      onJsonRpc: (map) {
        _dispatchEvent(map);
      },
      onConnectionChange: (connected) {
        ref.read(wsConnectedProvider.notifier).state = connected;
      },
    );

    _ws?.connect(wsUrl, sessionId: widget.sessionId, token: config.token);
  }

  void _dispatchEvent(Map<String, dynamic> map) {
    final type = map['type'] as String?;
    if (type == null) return;

    final sameSession = (map['sessionId'] as String? ?? '') == widget.sessionId;
    switch (type) {
      case 'chat.message':
        if (!sameSession) return;
        final msg = ChatMessage.fromJson(map);
        ref.read(chatMessagesProvider.notifier).addOrUpdateMessage(msg);
        if (msg.streaming) _scrollToBottom();
        break;
      case 'approval.requested':
        if (!sameSession) return;
        ref.read(pendingApprovalsProvider.notifier).addApproval(PendingApproval.fromJson(map));
        break;
      case 'approval.resolved':
        final id = map['approvalId'] as String? ?? '';
        ref.read(pendingApprovalsProvider.notifier).resolve(id, map['approved'] as bool? ?? false);
        break;
      case 'question.asked':
        if (!sameSession) return;
        ref.read(pendingQuestionsProvider.notifier).addQuestion(PendingQuestion.fromJson(map));
        break;
      case 'question.answered':
        final id = map['questionId'] as String? ?? '';
        ref.read(pendingQuestionsProvider.notifier).resolve(id);
        break;
      case 'auth.url':
        if (!sameSession) return;
        ref.read(authUrlCardsProvider.notifier).addCard(AuthUrlCard.fromJson(map));
        break;
      case 'session.status':
        ref.read(sessionsProvider.notifier).refresh();
        break;
    }
  }

  @override
  void dispose() {
    _ws?.disconnect();
    _textController.dispose();
    _scrollController.dispose();
    super.dispose();
  }

  void _scrollToBottom() {
    if (_scrollController.hasClients) {
      _scrollController.animateTo(
        _scrollController.position.maxScrollExtent + 80,
        duration: const Duration(milliseconds: 200),
        curve: Curves.easeOut,
      );
    }
  }

  void _sendMessage() async {
    final text = _textController.text.trim();
    if (text.isEmpty) return;

    _textController.clear();
    ref.read(chatMessagesProvider.notifier).appendUserMessage(widget.sessionId, text);
    _scrollToBottom();

    final api = ref.read(apiServiceProvider);
    try {
      await api.sendPrompt(widget.sessionId, text);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to send prompt: $e'), backgroundColor: AppTheme.dangerRed),
        );
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final session = ref.watch(activeSessionProvider);
    final messages = ref.watch(chatMessagesProvider);
    final approvals = ref.watch(pendingApprovalsProvider);
    final questions = ref.watch(pendingQuestionsProvider);
    final authCards = ref.watch(authUrlCardsProvider);
    final isConnected = ref.watch(wsConnectedProvider);

    return Scaffold(
      appBar: AppBar(
        titleSpacing: 0,
        title: Row(
          children: [
            Container(
              width: 8,
              height: 8,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: isConnected ? AppTheme.successGreen : AppTheme.dangerRed,
              ),
            ),
            const SizedBox(width: 8),
            Text(
              session?.agentId ?? 'Chat',
              style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 16),
            ),
            const SizedBox(width: 8),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
              decoration: BoxDecoration(
                color: AppTheme.surfaceDark,
                borderRadius: BorderRadius.circular(4),
                border: Border.all(color: AppTheme.borderDark),
              ),
              child: Text(
                session?.shortId ?? widget.sessionId,
                style: GoogleFonts.jetBrainsMono(fontSize: 11, color: AppTheme.textMuted),
              ),
            ),
          ],
        ),
        actions: [
          IconButton(
            tooltip: 'Terminal Tab',
            icon: const Icon(Icons.terminal, color: AppTheme.textMain),
            onPressed: () {
              context.push('/session/${widget.sessionId}/terminal');
            },
          ),
          IconButton(
            tooltip: 'Diff View',
            icon: const Icon(Icons.difference_outlined, color: AppTheme.textMain),
            onPressed: () => _showDiffModal(),
          ),
        ],
      ),
      body: Column(
        children: [
          // Auth / Login Cards
          ...authCards.map((card) => _buildAuthUrlCard(card)),

          // Pending Approval Cards
          ...approvals.where((a) => !a.resolved).map(
            (app) => _buildApprovalCard(app),
          ),

          // Pending Question Cards
          ...questions.where((q) => !q.resolved).map(
            (q) => _buildQuestionCard(q),
          ),

          // Message Transcript
          Expanded(
            child: messages.isEmpty
                ? _buildEmptyState(session)
                : ListView.builder(
                    controller: _scrollController,
                    padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
                    itemCount: messages.length,
                    itemBuilder: (context, idx) {
                      final msg = messages[idx];
                      return _buildMessageItem(msg);
                    },
                  ),
          ),

          // Input Bar
          _buildInputBar(),
        ],
      ),
    );
  }

  Widget _buildEmptyState(SessionItem? session) {
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Container(
            width: 56,
            height: 56,
            decoration: BoxDecoration(
              color: AppTheme.purpleGlow,
              borderRadius: BorderRadius.circular(16),
              border: Border.all(color: AppTheme.purpleAccent.withAlpha(76)),
            ),
            child: const Icon(Icons.auto_awesome, color: AppTheme.purpleAccent, size: 28),
          ),
          const SizedBox(height: 16),
          Text(
            'Ready to collaborate',
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: 6),
          Text(
            session != null ? 'Working in: ${session.cwd}' : 'Ask anything to get started',
            style: const TextStyle(color: AppTheme.textMuted, fontSize: 13),
          ),
        ],
      ),
    );
  }

  Widget _buildMessageItem(ChatMessage msg) {
    if (msg.isUser) {
      return Align(
        alignment: Alignment.centerRight,
        child: Container(
          margin: const EdgeInsets.only(bottom: 12, left: 48),
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          decoration: BoxDecoration(
            color: const Color(0xFF2E1065),
            borderRadius: BorderRadius.circular(16),
            border: Border.all(color: AppTheme.purpleAccent.withAlpha(100)),
          ),
          child: Text(
            msg.text,
            style: const TextStyle(color: Colors.white, fontSize: 14.5, height: 1.4),
          ),
        ),
      );
    }

    return Align(
      alignment: Alignment.centerLeft,
      child: Container(
        margin: const EdgeInsets.only(bottom: 16, right: 32),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (msg.isTool)
              Container(
                margin: const EdgeInsets.only(bottom: 6),
                padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                decoration: BoxDecoration(
                  color: AppTheme.cardDark,
                  borderRadius: BorderRadius.circular(6),
                  border: Border.all(color: AppTheme.borderDark),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const Icon(Icons.construction, size: 13, color: AppTheme.textMuted),
                    const SizedBox(width: 6),
                    Text(
                      msg.kind == 'tool_use' ? 'Tool Use' : 'Tool Result',
                      style: GoogleFonts.jetBrainsMono(fontSize: 11, color: AppTheme.textMuted),
                    ),
                  ],
                ),
              ),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
              decoration: BoxDecoration(
                color: AppTheme.surfaceDark,
                borderRadius: BorderRadius.circular(14),
                border: Border.all(color: AppTheme.borderDark),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  MarkdownBody(
                    data: msg.text,
                    styleSheet: MarkdownStyleSheet(
                      p: const TextStyle(color: AppTheme.textMain, fontSize: 14.5, height: 1.5),
                      code: GoogleFonts.jetBrainsMono(
                        backgroundColor: AppTheme.cardDark,
                        fontSize: 13,
                        color: AppTheme.textMain,
                      ),
                      codeblockDecoration: BoxDecoration(
                        color: AppTheme.bgDark,
                        borderRadius: BorderRadius.circular(8),
                        border: Border.all(color: AppTheme.borderDark),
                      ),
                    ),
                  ),
                  if (msg.streaming)
                    Padding(
                      padding: const EdgeInsets.only(top: 8),
                      child: Row(
                        children: [
                          Container(
                            width: 6,
                            height: 6,
                            decoration: const BoxDecoration(
                              shape: BoxShape.circle,
                              color: AppTheme.purpleAccent,
                            ),
                          ),
                          const SizedBox(width: 6),
                          const Text(
                            'Streaming...',
                            style: TextStyle(color: AppTheme.purpleAccent, fontSize: 11, fontWeight: FontWeight.w600),
                          ),
                        ],
                      ),
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildQuestionCard(PendingQuestion q) {
    return _QuestionCard(
      question: q,
      onAnswer: (answers) async {
        try {
          await ref.read(apiServiceProvider).sendAnswer(q.id, answers);
          ref.read(pendingQuestionsProvider.notifier).resolve(q.id);
        } catch (e) {
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: Text('Failed to send answer: $e'), backgroundColor: AppTheme.dangerRed),
            );
          }
        }
      },
    );
  }

  Widget _buildAuthUrlCard(AuthUrlCard card) {
    return Container(
      margin: const EdgeInsets.all(12),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: AppTheme.surfaceDark,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTheme.warningAmber, width: 1.5),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(Icons.key_outlined, color: AppTheme.warningAmber, size: 18),
              const SizedBox(width: 8),
              const Text(
                'Login Required',
                style: TextStyle(fontWeight: FontWeight.w700, fontSize: 14, color: AppTheme.textMain),
              ),
              const Spacer(),
              IconButton(
                icon: const Icon(Icons.copy, size: 15, color: AppTheme.textMuted),
                tooltip: 'Copy login URL',
                onPressed: () async {
                  await Clipboard.setData(ClipboardData(text: card.url));
                  if (mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(content: Text('Login URL copied to clipboard')),
                    );
                  }
                },
              ),
            ],
          ),
          const SizedBox(height: 4),
          const Text(
            'The agent is waiting for OAuth login. Open this URL on any device:',
            style: TextStyle(color: AppTheme.textMuted, fontSize: 12.5),
          ),
          const SizedBox(height: 8),
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(10),
            decoration: BoxDecoration(
              color: AppTheme.bgDark,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: AppTheme.borderDark),
            ),
            child: Text(
              card.url,
              style: GoogleFonts.jetBrainsMono(fontSize: 11.5, color: AppTheme.purpleLight),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildApprovalCard(PendingApproval app) {
    return Container(
      margin: const EdgeInsets.all(12),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: AppTheme.surfaceDark,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTheme.purpleAccent, width: 1.5),
        boxShadow: const [
          BoxShadow(color: AppTheme.purpleGlow, blurRadius: 12, spreadRadius: 1),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(Icons.shield_outlined, color: AppTheme.purpleLight, size: 18),
              const SizedBox(width: 8),
              const Text(
                'Approval Requested',
                style: TextStyle(fontWeight: FontWeight.w700, fontSize: 14, color: AppTheme.textMain),
              ),
              const Spacer(),
              Text(
                app.toolName,
                style: GoogleFonts.jetBrainsMono(fontSize: 11, color: AppTheme.textMuted),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(10),
            decoration: BoxDecoration(
              color: AppTheme.bgDark,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: AppTheme.borderDark),
            ),
            child: Text(
              app.command,
              style: GoogleFonts.jetBrainsMono(fontSize: 12, color: AppTheme.textMain),
            ),
          ),
          const SizedBox(height: 12),
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              OutlinedButton(
                onPressed: () {
                  ref.read(apiServiceProvider).sendApproval(app.id, false);
                  ref.read(pendingApprovalsProvider.notifier).resolve(app.id, false);
                },
                child: const Text('Deny', style: TextStyle(color: AppTheme.dangerRed)),
              ),
              const SizedBox(width: 10),
              ElevatedButton.icon(
                icon: const Icon(Icons.check, size: 16),
                label: const Text('Allow'),
                onPressed: () {
                  ref.read(apiServiceProvider).sendApproval(app.id, true);
                  ref.read(pendingApprovalsProvider.notifier).resolve(app.id, true);
                },
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _buildInputBar() {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      decoration: const BoxDecoration(
        color: AppTheme.bgDark,
        border: Border(top: BorderSide(color: AppTheme.borderDark)),
      ),
      child: SafeArea(
        child: Row(
          children: [
            Expanded(
              child: TextField(
                controller: _textController,
                maxLines: 4,
                minLines: 1,
                textInputAction: TextInputAction.send,
                onSubmitted: (_) => _sendMessage(),
                style: const TextStyle(fontSize: 14, color: AppTheme.textMain),
                decoration: const InputDecoration(
                  hintText: 'Ask a question or provide guidance...',
                  contentPadding: EdgeInsets.symmetric(horizontal: 14, vertical: 10),
                ),
              ),
            ),
            const SizedBox(width: 8),
            IconButton.filled(
              style: IconButton.styleFrom(
                backgroundColor: AppTheme.purpleAccent,
                foregroundColor: Colors.white,
                shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
              ),
              icon: const Icon(Icons.arrow_upward, size: 20),
              onPressed: _sendMessage,
            ),
          ],
        ),
      ),
    );
  }

  void _showDiffModal() async {
    final diff = await ref.read(apiServiceProvider).getDiff(widget.sessionId);
    if (!mounted) return;

    showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppTheme.surfaceDark,
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(16))),
      builder: (ctx) {
        return Container(
          height: MediaQuery.of(ctx).size.height * 0.75,
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  const Text('Workspace Changes (Git Diff)', style: TextStyle(fontWeight: FontWeight.w700, fontSize: 16)),
                  IconButton(icon: const Icon(Icons.close), onPressed: () => Navigator.pop(ctx)),
                ],
              ),
              const Divider(),
              Expanded(
                child: diff.isEmpty
                    ? const Center(child: Text('No unstaged changes in workspace', style: TextStyle(color: AppTheme.textMuted)))
                    : SingleChildScrollView(
                        child: Container(
                          width: double.infinity,
                          padding: const EdgeInsets.all(10),
                          decoration: BoxDecoration(
                            color: AppTheme.bgDark,
                            borderRadius: BorderRadius.circular(8),
                            border: Border.all(color: AppTheme.borderDark),
                          ),
                          child: _ColoredDiffText(diff: diff),
                        ),
                      ),
              ),
            ],
          ),
        );
      },
    );
  }
}

/// Renders unified diff output with conventional coloring: additions in
/// green, deletions in red, hunk headers in purple, file headers bold.
class _ColoredDiffText extends StatelessWidget {
  final String diff;
  const _ColoredDiffText({required this.diff});

  @override
  Widget build(BuildContext context) {
    final lines = diff.split('\n');
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (var i = 0; i < lines.length; i++)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 1),
            child: Text(
              lines[i],
              style: GoogleFonts.jetBrainsMono(
                fontSize: 12,
                height: 1.45,
                color: _diffLineColor(lines[i]),
                fontWeight: _diffLineWeight(lines[i]),
              ),
            ),
          ),
      ],
    );
  }

  Color _diffLineColor(String line) {
    if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('diff ')) {
      return AppTheme.textMuted;
    }
    if (line.startsWith('@@')) return AppTheme.purpleLight;
    if (line.startsWith('+')) return AppTheme.successGreen;
    if (line.startsWith('-')) return AppTheme.dangerRed;
    return AppTheme.textMain;
  }

  FontWeight _diffLineWeight(String line) {
    if (line.startsWith('diff ') || line.startsWith('@@')) return FontWeight.w700;
    return FontWeight.w400;
  }
}

/// Interactive disambiguation question card: single-select radio list plus a
/// write-in field, mirroring the agent's in-terminal option dialog.
class _QuestionCard extends StatefulWidget {
  final PendingQuestion question;
  final Future<void> Function(List<dynamic> answers) onAnswer;

  const _QuestionCard({required this.question, required this.onAnswer});

  @override
  State<_QuestionCard> createState() => _QuestionCardState();
}

class _QuestionCardState extends State<_QuestionCard> {
  int? _selected;
  final TextEditingController _customController = TextEditingController();
  bool _useCustom = false;

  @override
  void dispose() {
    _customController.dispose();
    super.dispose();
  }

  void _submit() {
    final answer = _useCustom
        ? _customController.text.trim()
        : (_selected != null ? widget.question.options[_selected!] : null);
    if (answer == null || answer.isEmpty) return;
    widget.onAnswer([answer]);
  }

  @override
  Widget build(BuildContext context) {
    final q = widget.question;
    final canSubmit = _useCustom
        ? _customController.text.trim().isNotEmpty
        : _selected != null;

    return Container(
      margin: const EdgeInsets.all(12),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: AppTheme.surfaceDark,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTheme.purpleAccent, width: 1.5),
        boxShadow: const [
          BoxShadow(color: AppTheme.purpleGlow, blurRadius: 12, spreadRadius: 1),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(Icons.help_outline, color: AppTheme.purpleLight, size: 18),
              const SizedBox(width: 8),
              const Text(
                'Question',
                style: TextStyle(fontWeight: FontWeight.w700, fontSize: 14, color: AppTheme.textMain),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Text(
            q.questionText,
            style: const TextStyle(color: AppTheme.textMain, fontSize: 13.5, height: 1.4),
          ),
          const SizedBox(height: 10),
          RadioGroup<int>(
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
                ...List.generate(q.options.length, (idx) {
                  return RadioListTile<int>(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    visualDensity: VisualDensity.compact,
                    value: idx,
                    title: Text(
                      q.options[idx],
                      style: const TextStyle(color: AppTheme.textMain, fontSize: 13),
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
                          color: _useCustom ? AppTheme.purpleAccent : AppTheme.textMuted,
                        ),
                        const SizedBox(width: 12),
                        Expanded(
                          child: TextField(
                            controller: _customController,
                            style: const TextStyle(color: AppTheme.textMain, fontSize: 13),
                            decoration: const InputDecoration(
                              hintText: 'Write your own answer...',
                              isDense: true,
                              border: InputBorder.none,
                              contentPadding: EdgeInsets.symmetric(vertical: 8),
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
          const SizedBox(height: 10),
          Align(
            alignment: Alignment.centerRight,
            child: ElevatedButton.icon(
              icon: const Icon(Icons.send, size: 15),
              label: const Text('Answer'),
              onPressed: canSubmit ? _submit : null,
            ),
          ),
        ],
      ),
    );
  }
}
