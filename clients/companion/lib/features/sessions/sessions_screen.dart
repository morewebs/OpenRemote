import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:google_fonts/google_fonts.dart';
import '../../models/models.dart';
import '../../providers/providers.dart';
import '../../theme/theme.dart';

class SessionsScreen extends ConsumerWidget {
  const SessionsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final sessionsAsync = ref.watch(sessionsProvider);

    return Scaffold(
      appBar: AppBar(
        title: const Text('OpenRemote', style: TextStyle(fontWeight: FontWeight.w800, fontSize: 18)),
        actions: [
          IconButton(
            tooltip: 'Refresh',
            icon: const Icon(Icons.refresh),
            onPressed: () => ref.read(sessionsProvider.notifier).refresh(),
          ),
          IconButton(
            tooltip: 'Settings',
            icon: const Icon(Icons.tune_outlined),
            onPressed: () => context.push('/settings'),
          ),
        ],
      ),
      body: sessionsAsync.when(
        loading: () => const Center(child: CircularProgressIndicator(color: AppTheme.purpleAccent)),
        error: (err, _) => Center(
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                const Icon(Icons.cloud_off, color: AppTheme.textMuted, size: 48),
                const SizedBox(height: 16),
                const Text('Cannot connect to OpenRemote daemon', style: TextStyle(fontWeight: FontWeight.w600, fontSize: 16)),
                const SizedBox(height: 8),
                Text('$err', style: const TextStyle(color: AppTheme.textMuted, fontSize: 12), textAlign: TextAlign.center),
                const SizedBox(height: 16),
                ElevatedButton(
                  onPressed: () => ref.read(sessionsProvider.notifier).refresh(),
                  child: const Text('Retry Connection'),
                ),
              ],
            ),
          ),
        ),
        data: (sessions) {
          if (sessions.isEmpty) {
            return Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Container(
                    width: 64,
                    height: 64,
                    decoration: BoxDecoration(
                      color: AppTheme.surfaceDark,
                      borderRadius: BorderRadius.circular(20),
                      border: Border.all(color: AppTheme.borderDark),
                    ),
                    child: const Icon(Icons.chat_bubble_outline, color: AppTheme.purpleAccent, size: 28),
                  ),
                  const SizedBox(height: 16),
                  const Text('No Active Sessions', style: TextStyle(fontWeight: FontWeight.w700, fontSize: 17)),
                  const SizedBox(height: 6),
                  const Text('Launch an AI coding assistant session to begin', style: TextStyle(color: AppTheme.textMuted, fontSize: 13)),
                  const SizedBox(height: 20),
                  ElevatedButton.icon(
                    icon: const Icon(Icons.add, size: 18),
                    label: const Text('New Session'),
                    onPressed: () => _showNewSessionModal(context, ref),
                  ),
                ],
              ),
            );
          }

          return ListView.builder(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
            itemCount: sessions.length,
            itemBuilder: (context, idx) {
              final s = sessions[idx];
              return _buildSessionCard(context, ref, s);
            },
          );
        },
      ),
      floatingActionButton: FloatingActionButton.extended(
        backgroundColor: AppTheme.purpleAccent,
        foregroundColor: Colors.white,
        icon: const Icon(Icons.add),
        label: const Text('New Session', style: TextStyle(fontWeight: FontWeight.w600)),
        onPressed: () => _showNewSessionModal(context, ref),
      ),
    );
  }

  Widget _buildSessionCard(BuildContext context, WidgetRef ref, SessionItem s) {
    return Container(
      margin: const EdgeInsets.only(bottom: 10),
      child: Card(
        child: InkWell(
          borderRadius: BorderRadius.circular(12),
          onTap: () {
            ref.read(activeSessionIdProvider.notifier).state = s.sessionId;
            context.push('/session/${s.sessionId}');
          },
          child: Padding(
            padding: const EdgeInsets.all(14),
            child: Row(
              children: [
                Container(
                  width: 42,
                  height: 42,
                  decoration: BoxDecoration(
                    color: AppTheme.bgDark,
                    borderRadius: BorderRadius.circular(10),
                    border: Border.all(color: AppTheme.borderDark),
                  ),
                  child: Icon(
                    _agentIcon(s.agentId),
                    color: s.isRunning ? AppTheme.purpleLight : AppTheme.textMuted,
                    size: 22,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Text(
                            s.agentId,
                            style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 15),
                          ),
                          const SizedBox(width: 8),
                          Container(
                            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                            decoration: BoxDecoration(
                              color: s.isRunning ? AppTheme.successGreen.withAlpha(38) : AppTheme.cardDark,
                              borderRadius: BorderRadius.circular(4),
                            ),
                            child: Text(
                              s.status,
                              style: TextStyle(
                                fontSize: 10,
                                fontWeight: FontWeight.w700,
                                color: s.isRunning ? AppTheme.successGreen : AppTheme.textMuted,
                              ),
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 4),
                      Text(
                        s.cwd,
                        style: GoogleFonts.jetBrainsMono(fontSize: 12, color: AppTheme.textMuted),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ],
                  ),
                ),
                IconButton(
                  icon: const Icon(Icons.delete_outline, size: 18, color: AppTheme.textMuted),
                  onPressed: () {
                    ref.read(sessionsProvider.notifier).deleteSession(s.sessionId);
                  },
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  IconData _agentIcon(String agentId) {
    switch (agentId) {
      case 'claude-code':
        return Icons.terminal;
      case 'antigravity':
        return Icons.rocket_launch_outlined;
      case 'opencode':
        return Icons.code;
      case 'codex':
        return Icons.psychology;
      case 'pi':
        return Icons.bolt;
      default:
        return Icons.desktop_windows;
    }
  }

  void _showNewSessionModal(BuildContext context, WidgetRef ref) {
    showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppTheme.surfaceDark,
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(16))),
      builder: (context) => const _NewSessionSheet(),
    );
  }
}

class _NewSessionSheet extends ConsumerStatefulWidget {
  const _NewSessionSheet();

  @override
  ConsumerState<_NewSessionSheet> createState() => _NewSessionSheetState();
}

class _NewSessionSheetState extends ConsumerState<_NewSessionSheet> {
  String _selectedAgent = 'claude-code';
  final TextEditingController _cwdController = TextEditingController(text: '.');
  final TextEditingController _taskController = TextEditingController();
  bool _useWorktree = false;
  bool _isLoading = false;

  @override
  void dispose() {
    _cwdController.dispose();
    _taskController.dispose();
    super.dispose();
  }

  void _createSession() async {
    setState(() => _isLoading = true);
    final s = await ref.read(sessionsProvider.notifier).createSession(
      agentId: _selectedAgent,
      cwd: _cwdController.text.trim(),
      useWorktree: _useWorktree,
      taskName: _taskController.text.trim(),
    );
    setState(() => _isLoading = false);

    if (s != null && mounted) {
      Navigator.pop(context);
      context.push('/session/${s.sessionId}');
    }
  }

  @override
  Widget build(BuildContext context) {
    final agentsAsync = ref.watch(agentsProvider);

    return Padding(
      padding: EdgeInsets.only(
        left: 20,
        right: 20,
        top: 20,
        bottom: MediaQuery.of(context).viewInsets.bottom + 24,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text('Launch Agent Session', style: TextStyle(fontWeight: FontWeight.w700, fontSize: 17)),
              IconButton(icon: const Icon(Icons.close), onPressed: () => Navigator.pop(context)),
            ],
          ),
          const SizedBox(height: 16),
          const Text('Select Assistant', style: TextStyle(color: AppTheme.textMuted, fontSize: 12, fontWeight: FontWeight.w600)),
          const SizedBox(height: 8),
          agentsAsync.when(
            loading: () => const LinearProgressIndicator(color: AppTheme.purpleAccent),
            error: (err, stack) => const Text('Error loading agents', style: TextStyle(color: AppTheme.dangerRed)),
            data: (agents) {
              final list = agents.isNotEmpty
                  ? agents
                  : [
                      AgentInfo(id: 'claude-code', displayName: 'Claude Code', available: true, supportsTerminal: true, supportsChatNative: true, supportsApproval: true, supportsDiff: true),
                      AgentInfo(id: 'antigravity', displayName: 'Antigravity', available: true, supportsTerminal: true, supportsChatNative: true, supportsApproval: true, supportsDiff: true),
                      AgentInfo(id: 'opencode', displayName: 'OpenCode', available: true, supportsTerminal: true, supportsChatNative: true, supportsApproval: true, supportsDiff: true),
                      AgentInfo(id: 'codex', displayName: 'Codex', available: true, supportsTerminal: true, supportsChatNative: true, supportsApproval: true, supportsDiff: true),
                      AgentInfo(id: 'shell', displayName: 'Shell', available: true, supportsTerminal: true, supportsChatNative: false, supportsApproval: false, supportsDiff: false),
                    ];
              return SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  children: list.map((a) {
                    final selected = _selectedAgent == a.id;
                    return Padding(
                      padding: const EdgeInsets.only(right: 8),
                      child: ChoiceChip(
                        selected: selected,
                        label: Text(a.displayName),
                        selectedColor: AppTheme.purpleAccent,
                        backgroundColor: AppTheme.cardDark,
                        labelStyle: TextStyle(
                          color: selected ? Colors.white : AppTheme.textMain,
                          fontWeight: selected ? FontWeight.w700 : FontWeight.normal,
                          fontSize: 13,
                        ),
                        onSelected: (val) {
                          if (val) setState(() => _selectedAgent = a.id);
                        },
                      ),
                    );
                  }).toList(),
                ),
              );
            },
          ),
          const SizedBox(height: 16),
          const Text('Working Directory', style: TextStyle(color: AppTheme.textMuted, fontSize: 12, fontWeight: FontWeight.w600)),
          const SizedBox(height: 6),
          TextField(
            controller: _cwdController,
            style: GoogleFonts.jetBrainsMono(fontSize: 13),
            decoration: const InputDecoration(
              hintText: 'e.g. /home/user/project or .',
              prefixIcon: Icon(Icons.folder_open, size: 18),
            ),
          ),
          const SizedBox(height: 14),
          Row(
            children: [
              Checkbox(
                value: _useWorktree,
                activeColor: AppTheme.purpleAccent,
                onChanged: (v) => setState(() => _useWorktree = v ?? false),
              ),
              const Text('Isolate with Git Worktree', style: TextStyle(fontSize: 13.5)),
            ],
          ),
          if (_useWorktree) ...[
            const SizedBox(height: 8),
            TextField(
              controller: _taskController,
              decoration: const InputDecoration(
                hintText: 'Task branch name (e.g. add-auth-api)',
              ),
            ),
          ],
          const SizedBox(height: 20),
          SizedBox(
            width: double.infinity,
            height: 44,
            child: ElevatedButton(
              onPressed: _isLoading ? null : _createSession,
              child: _isLoading
                  ? const SizedBox(width: 20, height: 20, child: CircularProgressIndicator(color: Colors.white, strokeWidth: 2))
                  : const Text('Start Session', style: TextStyle(fontSize: 15)),
            ),
          ),
        ],
      ),
    );
  }
}
