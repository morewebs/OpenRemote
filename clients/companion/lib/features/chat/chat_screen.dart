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
import 'widgets/question_card.dart';

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
  bool _sending = false;
  final Set<String> _resolvingApprovals = {};
  late final ProviderSubscription<ServerConfig> _configSubscription;

  @override
  void initState() {
    super.initState();
    _configSubscription = ref.listenManual(serverConfigProvider, (
      previous,
      next,
    ) {
      if (!mounted ||
          (previous?.baseUrl == next.baseUrl && previous?.token == next.token))
        return;
      ref.read(chatMessagesProvider(widget.sessionId).notifier).clear();
      ref.read(pendingApprovalsProvider(widget.sessionId).notifier).clear();
      ref.read(pendingQuestionsProvider(widget.sessionId).notifier).clear();
      ref.read(authUrlCardsProvider(widget.sessionId).notifier).clear();
      ref.read(diffCardsProvider(widget.sessionId).notifier).clear();
      ref.read(turnSummariesProvider(widget.sessionId).notifier).clear();
      ref.read(artifactCardsProvider(widget.sessionId).notifier).clear();
      _initWebSocket();
    });
    Future.microtask(() {
      if (!mounted) return;
      ref.read(activeSessionIdProvider.notifier).state = widget.sessionId;
      _initWebSocket();
    });
  }

  void _initWebSocket() {
    _ws?.disconnect();
    final config = ref.read(serverConfigProvider);
    final wsBase = config.baseUrl.replaceFirst('http', 'ws');
    final wsUrl = '$wsBase/ws';

    _ws = WebSocketService(
      onJsonRpc: (map) {
        _dispatchEvent(map);
      },
      onConnectionChange: (connected) {
        if (!mounted) return;
        ref.read(wsConnectedProvider.notifier).state = connected;
      },
    );

    _ws?.connect(wsUrl, sessionId: widget.sessionId, token: config.token);
  }

  void _dispatchEvent(Map<String, dynamic> map) {
    final type = map['type'] as String?;
    if (type == null) return;

    final evtSessionId = map['sessionId'] as String? ?? '';
    final sameSession = evtSessionId == widget.sessionId;

    switch (type) {
      case 'chat.message':
        if (!sameSession) return;
        ref
            .read(chatMessagesProvider(widget.sessionId).notifier)
            .addOrUpdateMessage(ChatMessage.fromJson(map));
        if (map['streaming'] == true) _scrollToBottom();
        break;
      case 'approval.requested':
        if (!sameSession) return;
        ref
            .read(pendingApprovalsProvider(widget.sessionId).notifier)
            .addApproval(PendingApproval.fromJson(map));
        break;
      case 'approval.resolved':
        {
          final targetSid = evtSessionId.isNotEmpty
              ? evtSessionId
              : widget.sessionId;
          final id = map['approvalId'] as String? ?? '';
          ref
              .read(pendingApprovalsProvider(targetSid).notifier)
              .resolve(id, map['approved'] as bool? ?? false);
        }
        break;
      case 'question.asked':
        if (!sameSession) return;
        ref
            .read(pendingQuestionsProvider(widget.sessionId).notifier)
            .addQuestion(PendingQuestion.fromJson(map));
        break;
      case 'question.answered':
        {
          final targetSid = evtSessionId.isNotEmpty
              ? evtSessionId
              : widget.sessionId;
          final id = map['questionId'] as String? ?? '';
          ref.read(pendingQuestionsProvider(targetSid).notifier).resolve(id);
        }
        break;
      case 'auth.url':
        if (!sameSession) return;
        ref
            .read(authUrlCardsProvider(widget.sessionId).notifier)
            .addCard(AuthUrlCard.fromJson(map));
        break;
      case 'diff.generated':
        if (!sameSession) return;
        ref
            .read(diffCardsProvider(widget.sessionId).notifier)
            .addCard(DiffCard.fromJson(map));
        _scrollToBottom();
        break;
      case 'turn.completed':
        if (!sameSession) return;
        ref
            .read(turnSummariesProvider(widget.sessionId).notifier)
            .addSummary(TurnSummary.fromJson(map));
        _scrollToBottom();
        break;
      case 'artifact.updated':
        if (!sameSession) return;
        ref
            .read(artifactCardsProvider(widget.sessionId).notifier)
            .addOrUpdateArtifact(ArtifactCard.fromJson(map));
        _scrollToBottom();
        break;
      case 'session.status':
        ref.read(sessionsProvider.notifier).refresh();
        break;
    }
  }

  @override
  void dispose() {
    _configSubscription.close();
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
    if (text.isEmpty || _sending) return;
    setState(() => _sending = true);

    final api = ref.read(apiServiceProvider);
    try {
      await api.sendPrompt(widget.sessionId, text);
      if (mounted && _textController.text.trim() == text) {
        _textController.clear();
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to send prompt: $e'),
            backgroundColor: AppTheme.dangerRed,
          ),
        );
      }
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  Future<void> _resolveApproval(PendingApproval app, bool approved) async {
    if (_resolvingApprovals.contains(app.id)) return;
    setState(() => _resolvingApprovals.add(app.id));
    try {
      await ref.read(apiServiceProvider).sendApproval(app.id, approved);
      if (mounted) {
        ref
            .read(pendingApprovalsProvider(widget.sessionId).notifier)
            .resolve(app.id, approved);
      }
    } catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Approval was not delivered: $error')),
        );
      }
    } finally {
      if (mounted) setState(() => _resolvingApprovals.remove(app.id));
    }
  }

  @override
  Widget build(BuildContext context) {
    final session = ref.watch(activeSessionProvider);
    final agents = ref.watch(agentsProvider).valueOrNull ?? const <AgentInfo>[];
    final supportsTerminal = agents.any(
      (agent) => agent.id == session?.agentId && agent.supportsTerminal,
    );
    final messages = ref.watch(chatMessagesProvider(widget.sessionId));
    final approvals = ref.watch(pendingApprovalsProvider(widget.sessionId));
    final questions = ref.watch(pendingQuestionsProvider(widget.sessionId));
    final authCards = ref.watch(authUrlCardsProvider(widget.sessionId));
    final diffCards = ref.watch(diffCardsProvider(widget.sessionId));
    final turnSummaries = ref.watch(turnSummariesProvider(widget.sessionId));
    final artifactCards = ref.watch(artifactCardsProvider(widget.sessionId));
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
            Flexible(
              child: Text(
                session?.agentId ?? 'Chat',
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(
                  fontWeight: FontWeight.w700,
                  fontSize: 16,
                ),
              ),
            ),
            const SizedBox(width: 8),
            if (MediaQuery.sizeOf(context).width >= 450)
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                decoration: BoxDecoration(
                  color: AppTheme.surfaceDark,
                  borderRadius: BorderRadius.circular(4),
                  border: Border.all(color: AppTheme.borderDark),
                ),
                child: Text(
                  session?.shortId ?? widget.sessionId,
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 11,
                    color: AppTheme.textMuted,
                  ),
                ),
              ),
          ],
        ),
        actions: [
          IconButton(
            tooltip: 'Workspace files',
            icon: const Icon(Icons.folder_open),
            onPressed: () => context.push('/session/${widget.sessionId}/files'),
          ),
          if (supportsTerminal)
            IconButton(
              tooltip: 'Terminal Tab',
              icon: const Icon(Icons.terminal, color: AppTheme.textMain),
              onPressed: () {
                context.push('/session/${widget.sessionId}/terminal');
              },
            ),
          IconButton(
            tooltip: 'Diff View',
            icon: const Icon(
              Icons.difference_outlined,
              color: AppTheme.textMain,
            ),
            onPressed: () =>
                context.push('/session/${widget.sessionId}/changes'),
          ),
        ],
      ),
      body: Column(
        children: [
          if (!isConnected)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
              child: Row(
                children: [
                  const Expanded(
                    child: Text(
                      'Disconnected. Reconnecting…',
                      style: TextStyle(color: AppTheme.textMuted),
                    ),
                  ),
                  TextButton(
                    onPressed: () => context.push('/settings'),
                    child: const Text('Settings'),
                  ),
                ],
              ),
            ),
          Expanded(
            child: ListView(
              controller: _scrollController,
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
              children: [
                // Auth / Login Cards
                ...authCards.map((card) => _buildAuthUrlCard(card)),

                // Pending Approval Cards
                ...approvals
                    .where((a) => !a.resolved)
                    .map((app) => _buildApprovalCard(app)),

                // Pending Question Cards
                ...questions
                    .where((q) => !q.resolved)
                    .map((q) => _buildQuestionCard(q)),

                // Artifact Updated Cards
                ...artifactCards.map((art) => _buildArtifactCard(art)),

                // Turn Completed Summaries
                ...turnSummaries.map((sum) => _buildTurnSummaryCard(sum)),

                // Live Generated Diff Cards
                ...diffCards.map((diff) => _buildDiffCard(diff)),

                // Message Transcript
                if (messages.isEmpty)
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 60),
                    child: _buildEmptyState(session),
                  ),
                ...messages.map(_buildMessageItem),
              ],
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
            child: const Icon(
              Icons.auto_awesome,
              color: AppTheme.purpleAccent,
              size: 28,
            ),
          ),
          const SizedBox(height: 16),
          Text(
            'Ready to collaborate',
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: 6),
          Text(
            session != null
                ? 'Working in: ${session.cwd}'
                : 'Ask anything to get started',
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
            style: const TextStyle(
              color: Colors.white,
              fontSize: 14.5,
              height: 1.4,
            ),
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
                padding: const EdgeInsets.symmetric(
                  horizontal: 10,
                  vertical: 4,
                ),
                decoration: BoxDecoration(
                  color: AppTheme.cardDark,
                  borderRadius: BorderRadius.circular(6),
                  border: Border.all(color: AppTheme.borderDark),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const Icon(
                      Icons.construction,
                      size: 13,
                      color: AppTheme.textMuted,
                    ),
                    const SizedBox(width: 6),
                    Text(
                      msg.kind == 'tool_use' ? 'Tool Use' : 'Tool Result',
                      style: GoogleFonts.jetBrainsMono(
                        fontSize: 11,
                        color: AppTheme.textMuted,
                      ),
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
                      p: const TextStyle(
                        color: AppTheme.textMain,
                        fontSize: 14.5,
                        height: 1.5,
                      ),
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
                            style: TextStyle(
                              color: AppTheme.purpleAccent,
                              fontSize: 11,
                              fontWeight: FontWeight.w600,
                            ),
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
    return QuestionCard(
      key: ValueKey(q.id),
      question: q,
      onAnswer: (answers) async {
        try {
          await ref.read(apiServiceProvider).sendAnswer(q.id, answers);
          if (mounted) {
            ref
                .read(pendingQuestionsProvider(widget.sessionId).notifier)
                .resolve(q.id);
          }
        } catch (e) {
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: Text('Failed to send answer: $e'),
                backgroundColor: AppTheme.dangerRed,
              ),
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
              const Icon(
                Icons.key_outlined,
                color: AppTheme.warningAmber,
                size: 18,
              ),
              const SizedBox(width: 8),
              const Text(
                'Login Required',
                style: TextStyle(
                  fontWeight: FontWeight.w700,
                  fontSize: 14,
                  color: AppTheme.textMain,
                ),
              ),
              const Spacer(),
              IconButton(
                icon: const Icon(
                  Icons.copy,
                  size: 15,
                  color: AppTheme.textMuted,
                ),
                tooltip: 'Copy login URL',
                onPressed: () async {
                  await Clipboard.setData(ClipboardData(text: card.url));
                  if (mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(
                        content: Text('Login URL copied to clipboard'),
                      ),
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
              style: GoogleFonts.jetBrainsMono(
                fontSize: 11.5,
                color: AppTheme.purpleLight,
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildDiffCard(DiffCard card) {
    return Container(
      margin: const EdgeInsets.all(12),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: AppTheme.surfaceDark,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: AppTheme.purpleAccent.withAlpha(120),
          width: 1.5,
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(
                Icons.difference_outlined,
                color: AppTheme.purpleLight,
                size: 18,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  card.filePath,
                  style: GoogleFonts.jetBrainsMono(
                    fontWeight: FontWeight.w700,
                    fontSize: 13,
                    color: AppTheme.textMain,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (card.additions > 0)
                Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 6,
                    vertical: 2,
                  ),
                  decoration: BoxDecoration(
                    color: AppTheme.successGreen.withAlpha(40),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: Text(
                    '+${card.additions}',
                    style: GoogleFonts.jetBrainsMono(
                      fontSize: 11,
                      color: AppTheme.successGreen,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                ),
              if (card.deletions > 0) ...[
                const SizedBox(width: 6),
                Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 6,
                    vertical: 2,
                  ),
                  decoration: BoxDecoration(
                    color: AppTheme.dangerRed.withAlpha(40),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: Text(
                    '-${card.deletions}',
                    style: GoogleFonts.jetBrainsMono(
                      fontSize: 11,
                      color: AppTheme.dangerRed,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                ),
              ],
            ],
          ),
          if (card.diffPatch.isNotEmpty) ...[
            const SizedBox(height: 8),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(10),
              decoration: BoxDecoration(
                color: AppTheme.bgDark,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: AppTheme.borderDark),
              ),
              child: _ColoredDiffText(diff: card.diffPatch),
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildTurnSummaryCard(TurnSummary summary) {
    return Container(
      margin: const EdgeInsets.all(12),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: AppTheme.surfaceDark,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: AppTheme.successGreen.withAlpha(120),
          width: 1.5,
        ),
      ),
      child: Row(
        children: [
          Container(
            padding: const EdgeInsets.all(6),
            decoration: BoxDecoration(
              color: AppTheme.successGreen.withAlpha(30),
              shape: BoxShape.circle,
            ),
            child: const Icon(
              Icons.check_circle_outline,
              color: AppTheme.successGreen,
              size: 18,
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  'Turn Completed',
                  style: TextStyle(
                    fontWeight: FontWeight.w700,
                    fontSize: 13.5,
                    color: AppTheme.textMain,
                  ),
                ),
                if (summary.summary != null && summary.summary!.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(top: 2),
                    child: Text(
                      summary.summary!,
                      style: const TextStyle(
                        color: AppTheme.textMuted,
                        fontSize: 12,
                      ),
                    ),
                  ),
              ],
            ),
          ),
          Column(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              if (summary.durationMs > 0)
                Text(
                  '${(summary.durationMs / 1000).toStringAsFixed(1)}s',
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 11.5,
                    color: AppTheme.textMuted,
                  ),
                ),
              if (summary.costUsd != null)
                Text(
                  '\$${summary.costUsd!.toStringAsFixed(4)}',
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 11.5,
                    color: AppTheme.purpleLight,
                    fontWeight: FontWeight.w600,
                  ),
                ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _buildArtifactCard(ArtifactCard art) {
    return Container(
      margin: const EdgeInsets.all(12),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: AppTheme.surfaceDark,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: AppTheme.purpleAccent.withAlpha(100),
          width: 1.5,
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(
                Icons.description_outlined,
                color: AppTheme.purpleAccent,
                size: 18,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  art.path,
                  style: GoogleFonts.jetBrainsMono(
                    fontWeight: FontWeight.w700,
                    fontSize: 13,
                    color: AppTheme.textMain,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                decoration: BoxDecoration(
                  color: AppTheme.cardDark,
                  borderRadius: BorderRadius.circular(4),
                ),
                child: Text(
                  art.kind,
                  style: GoogleFonts.jetBrainsMono(
                    fontSize: 11,
                    color: AppTheme.textMuted,
                  ),
                ),
              ),
            ],
          ),
          if (art.content.isNotEmpty) ...[
            const SizedBox(height: 8),
            Container(
              width: double.infinity,
              constraints: const BoxConstraints(maxHeight: 180),
              padding: const EdgeInsets.all(10),
              decoration: BoxDecoration(
                color: AppTheme.bgDark,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: AppTheme.borderDark),
              ),
              child: SingleChildScrollView(
                child: MarkdownBody(
                  data: art.content,
                  styleSheet: MarkdownStyleSheet(
                    p: const TextStyle(color: AppTheme.textMain, fontSize: 12),
                    code: GoogleFonts.jetBrainsMono(
                      fontSize: 11,
                      color: AppTheme.textMain,
                    ),
                  ),
                ),
              ),
            ),
          ],
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
          BoxShadow(
            color: AppTheme.purpleGlow,
            blurRadius: 12,
            spreadRadius: 1,
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(
                Icons.shield_outlined,
                color: AppTheme.purpleLight,
                size: 18,
              ),
              const SizedBox(width: 8),
              const Text(
                'Approval Requested',
                style: TextStyle(
                  fontWeight: FontWeight.w700,
                  fontSize: 14,
                  color: AppTheme.textMain,
                ),
              ),
              const Spacer(),
              Text(
                app.toolName,
                style: GoogleFonts.jetBrainsMono(
                  fontSize: 11,
                  color: AppTheme.textMuted,
                ),
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
              style: GoogleFonts.jetBrainsMono(
                fontSize: 12,
                color: AppTheme.textMain,
              ),
            ),
          ),
          const SizedBox(height: 12),
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              OutlinedButton(
                onPressed: _resolvingApprovals.contains(app.id)
                    ? null
                    : () => _resolveApproval(app, false),
                child: const Text(
                  'Deny',
                  style: TextStyle(color: AppTheme.dangerRed),
                ),
              ),
              const SizedBox(width: 10),
              ElevatedButton.icon(
                icon: const Icon(Icons.check, size: 16),
                label: const Text('Allow'),
                onPressed: _resolvingApprovals.contains(app.id)
                    ? null
                    : () => _resolveApproval(app, true),
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
                  contentPadding: EdgeInsets.symmetric(
                    horizontal: 14,
                    vertical: 10,
                  ),
                ),
              ),
            ),
            const SizedBox(width: 8),
            IconButton.filled(
              style: IconButton.styleFrom(
                backgroundColor: AppTheme.purpleAccent,
                foregroundColor: Colors.white,
                shape: RoundedRectangleBorder(
                  borderRadius: BorderRadius.circular(10),
                ),
              ),
              tooltip: 'Send message',
              icon: _sending
                  ? const SizedBox(
                      width: 20,
                      height: 20,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.arrow_upward, size: 20),
              onPressed: _sending ? null : _sendMessage,
            ),
          ],
        ),
      ),
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
    if (line.startsWith('+++') ||
        line.startsWith('---') ||
        line.startsWith('diff ')) {
      return AppTheme.textMuted;
    }
    if (line.startsWith('@@')) return AppTheme.purpleLight;
    if (line.startsWith('+')) return AppTheme.successGreen;
    if (line.startsWith('-')) return AppTheme.dangerRed;
    return AppTheme.textMain;
  }

  FontWeight _diffLineWeight(String line) {
    if (line.startsWith('diff ') || line.startsWith('@@')) {
      return FontWeight.w700;
    }
    return FontWeight.w400;
  }
}
