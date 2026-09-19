import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:google_fonts/google_fonts.dart';
import '../../providers/providers.dart';
import '../../theme/theme.dart';

/// Workspace inspection extends the companion's existing dark, purple-accented
/// shell. Files keep their hierarchy; changes offer unified and split views.
class WorkspaceScreen extends ConsumerStatefulWidget {
  final String sessionId;
  final bool showChanges;
  const WorkspaceScreen({
    super.key,
    required this.sessionId,
    this.showChanges = false,
  });
  @override
  ConsumerState<WorkspaceScreen> createState() => _WorkspaceScreenState();
}

class _WorkspaceScreenState extends ConsumerState<WorkspaceScreen> {
  final List<String> _history = [];
  List<Map<String, dynamic>> _files = [];
  String? _error;
  String _diff = '';
  bool _loading = true;
  bool _split = false;
  late bool _changes;
  int _request = 0;

  @override
  void initState() {
    super.initState();
    _changes = widget.showChanges;
    _load();
  }

  Future<void> _load() async {
    final request = ++_request;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final api = ref.read(apiServiceProvider);
      if (_changes) {
        final diff = await api.getDiff(widget.sessionId);
        if (!mounted || request != _request) return;
        _diff = diff;
      } else {
        if (_history.isEmpty) {
          final session = await api.getSession(widget.sessionId);
          if (!mounted || request != _request) return;
          _history.add(
            session.worktreePath?.isNotEmpty == true
                ? session.worktreePath!
                : session.cwd,
          );
        }
        final files = await api.getFiles(_history.last);
        if (!mounted || request != _request) return;
        _files = files;
      }
    } catch (error) {
      if (mounted && request == _request) _error = '$error';
    } finally {
      if (mounted && request == _request) setState(() => _loading = false);
    }
  }

  Future<void> _preview(Map<String, dynamic> entry) async {
    if (entry['isDir'] == true) {
      _history.add(entry['path'] as String);
      await _load();
      return;
    }
    final future = ref
        .read(apiServiceProvider)
        .getFile(entry['path'] as String);
    if (!mounted) return;
    await showDialog<void>(
      context: context,
      builder: (context) => Dialog(
        insetPadding: const EdgeInsets.all(20),
        child: SizedBox(
          width: 900,
          height: MediaQuery.sizeOf(context).height * .8,
          child: Column(
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(20, 12, 8, 8),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        entry['name'] as String,
                        style: const TextStyle(
                          fontSize: 17,
                          fontWeight: FontWeight.w600,
                        ),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                    IconButton(
                      tooltip: 'Close preview',
                      onPressed: () => Navigator.pop(context),
                      icon: const Icon(Icons.close),
                    ),
                  ],
                ),
              ),
              const Divider(height: 1),
              Expanded(
                child: FutureBuilder<Map<String, dynamic>>(
                  future: future,
                  builder: (context, snapshot) {
                    if (snapshot.hasError) {
                      return _errorPanel(
                        '${snapshot.error}',
                        () => Navigator.pop(context),
                        label: 'Close',
                      );
                    }
                    if (!snapshot.hasData) {
                      return const Center(child: CircularProgressIndicator());
                    }
                    final data = snapshot.data!;
                    return Column(
                      children: [
                        if (data['truncated'] == true)
                          const Padding(
                            padding: EdgeInsets.all(12),
                            child: Text('Preview limited to the first 256 KB.'),
                          ),
                        Expanded(
                          child: Scrollbar(
                            child: SingleChildScrollView(
                              padding: const EdgeInsets.all(20),
                              child: Align(
                                alignment: Alignment.topLeft,
                                child: SelectableText(
                                  data['content'] as String,
                                  style: GoogleFonts.jetBrainsMono(
                                    fontSize: 13,
                                    height: 1.6,
                                  ),
                                ),
                              ),
                            ),
                          ),
                        ),
                      ],
                    );
                  },
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _errorPanel(
    String error,
    VoidCallback retry, {
    String label = 'Try again',
  }) => Center(
    child: Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(Icons.error_outline, size: 32, color: AppTheme.dangerRed),
          const SizedBox(height: 12),
          Text(error, textAlign: TextAlign.center),
          const SizedBox(height: 16),
          OutlinedButton(onPressed: retry, child: Text(label)),
        ],
      ),
    ),
  );

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(
      title: const Text('Workspace'),
      actions: [
        IconButton(
          tooltip: 'Refresh workspace',
          onPressed: _loading ? null : _load,
          icon: const Icon(Icons.refresh),
        ),
      ],
    ),
    body: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
          child: SegmentedButton<bool>(
            segments: const [
              ButtonSegment(
                value: false,
                label: Text('Files'),
                icon: Icon(Icons.folder_outlined),
              ),
              ButtonSegment(
                value: true,
                label: Text('Changes'),
                icon: Icon(Icons.difference_outlined),
              ),
            ],
            selected: {_changes},
            onSelectionChanged: (value) {
              setState(() => _changes = value.first);
              _load();
            },
          ),
        ),
        if (!_changes && _history.isNotEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: Row(
              children: [
                IconButton(
                  tooltip: 'Parent folder',
                  onPressed: _history.length > 1 && !_loading
                      ? () {
                          _history.removeLast();
                          _load();
                        }
                      : null,
                  icon: const Icon(Icons.arrow_upward),
                ),
                Expanded(
                  child: Tooltip(
                    message: _history.last,
                    child: Text(
                      _history.last,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: GoogleFonts.jetBrainsMono(
                        fontSize: 12,
                        color: AppTheme.textMuted,
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        if (_changes && !_loading && _error == null && _diff.isNotEmpty)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    'Staged and unstaged tracked changes',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ),
                IconButton(
                  tooltip: 'Copy patch',
                  icon: const Icon(Icons.copy),
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: _diff));
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(content: Text('Patch copied.')),
                    );
                  },
                ),
                if (MediaQuery.sizeOf(context).width >= 700)
                  IconButton(
                    tooltip: _split ? 'Unified diff' : 'Split diff',
                    onPressed: () => setState(() => _split = !_split),
                    icon: Icon(
                      _split
                          ? Icons.view_agenda_outlined
                          : Icons.view_column_outlined,
                    ),
                  ),
              ],
            ),
          ),
        Expanded(
          child: _loading
              ? const Center(child: CircularProgressIndicator())
              : _error != null
              ? _errorPanel(_error!, _load)
              : _changes
              ? _diffView()
              : _fileList(),
        ),
      ],
    ),
  );

  Widget _fileList() {
    if (_files.isEmpty) {
      return const Center(child: Text('This folder is empty.'));
    }
    return ListView.separated(
      itemCount: _files.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
      itemBuilder: (context, index) {
        final entry = _files[index];
        final directory = entry['isDir'] == true;
        final bytes = (entry['size'] as num?)?.toInt() ?? 0;
        return ListTile(
          contentPadding: const EdgeInsets.symmetric(
            horizontal: 24,
            vertical: 4,
          ),
          leading: Icon(
            directory ? Icons.folder_outlined : Icons.description_outlined,
            color: directory ? AppTheme.purpleLight : AppTheme.textMuted,
          ),
          title: Text(
            entry['name'] as String,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
          ),
          trailing: directory
              ? const Icon(Icons.chevron_right)
              : Text(
                  bytes < 1024
                      ? '$bytes B'
                      : '${(bytes / 1024).toStringAsFixed(1)} KB',
                  style: const TextStyle(color: AppTheme.textMuted),
                ),
          onTap: () => _preview(entry),
        );
      },
    );
  }

  Widget _diffView() {
    if (_diff.isEmpty) {
      return const Center(
        child: Text('No tracked changes. New files are available in Files.'),
      );
    }
    final lines = _diff.split('\n');
    final split = _split && MediaQuery.sizeOf(context).width >= 700;
    return ListView.builder(
      itemCount: lines.length,
      itemBuilder: (context, index) {
        final line = lines[index];
        final added = line.startsWith('+') && !line.startsWith('+++');
        final removed = line.startsWith('-') && !line.startsWith('---');
        final heading =
            line.startsWith('@@') ||
            line.startsWith('diff ') ||
            line.startsWith('+++') ||
            line.startsWith('---');
        final color = added
            ? AppTheme.successGreen
            : removed
            ? AppTheme.dangerRed
            : heading
            ? AppTheme.purpleLight
            : AppTheme.textMain;
        Widget cell(String text) => Container(
          width: double.infinity,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 3),
          color: (added || removed)
              ? color.withValues(alpha: .08)
              : Colors.transparent,
          child: SelectableText(
            text.isEmpty ? ' ' : text,
            style: GoogleFonts.jetBrainsMono(
              fontSize: 12,
              height: 1.5,
              color: color,
            ),
          ),
        );
        if (!split || heading) return cell(line);
        return IntrinsicHeight(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Expanded(child: cell(added ? '' : line)),
              const VerticalDivider(width: 1),
              Expanded(child: cell(removed ? '' : line)),
            ],
          ),
        );
      },
    );
  }
}
