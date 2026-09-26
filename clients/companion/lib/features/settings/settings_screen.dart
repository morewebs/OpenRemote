import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:google_fonts/google_fonts.dart';
import '../../models/models.dart';
import '../../providers/providers.dart';
import '../../services/api_service.dart';
import '../../theme/theme.dart';

class SettingsScreen extends ConsumerStatefulWidget {
  const SettingsScreen({super.key});

  @override
  ConsumerState<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends ConsumerState<SettingsScreen> {
  late final TextEditingController _urlController;
  late final TextEditingController _tokenController;
  bool _testingHealth = false;
  bool? _healthOk;
  bool _applyingUpdate = false;

  @override
  void initState() {
    super.initState();
    final config = ref.read(serverConfigProvider);
    _urlController = TextEditingController(text: config.baseUrl);
    _tokenController = TextEditingController(text: config.token);
  }

  @override
  void dispose() {
    _urlController.dispose();
    _tokenController.dispose();
    super.dispose();
  }

  Future<void> _applyDaemonUpdate() async {
    final status = ref.read(updateStatusProvider).valueOrNull;
    final latest = status?.latest ?? '';
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Update daemon?'),
        content: Text(
          'The daemon will download $latest, verify its checksum, swap itself '
          'and restart. Live agent sessions stop; transcripts survive.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Update'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    setState(() => _applyingUpdate = true);
    final api = ref.read(apiServiceProvider);
    try {
      await api.applyUpdate();
      // The daemon shuts down and restarts under its supervisor; poll until
      // the update endpoint answers again, then report the outcome.
      DaemonUpdateStatus? after;
      for (var i = 0; i < 45; i++) {
        await Future<void>.delayed(const Duration(seconds: 2));
        after = await api.getUpdateStatus();
        if (after != null && !after.applying) break;
      }
      if (!mounted) return;
      final error = after?.applyError;
      final version = after?.current ?? '';
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            error != null && error.isNotEmpty
                ? 'Update failed: $error'
                : 'Daemon is now running version $version',
          ),
          backgroundColor:
              error != null && error.isNotEmpty ? AppTheme.dangerRed : AppTheme.purpleAccent,
        ),
      );
      ref.invalidate(updateStatusProvider);
    } catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Update request failed: $error')),
        );
      }
    } finally {
      if (mounted) setState(() => _applyingUpdate = false);
    }
  }

  void _testConnection() async {
    setState(() {
      _testingHealth = true;
      _healthOk = null;
    });
    final api = ApiService(
      baseUrl: _urlController.text.trim(),
      token: _tokenController.text.trim(),
    );
    final ok = await api.checkHealth();
    if (!mounted) return;
    setState(() {
      _testingHealth = false;
      _healthOk = ok;
    });
  }

  void _save() async {
    try {
      await ref
          .read(serverConfigProvider.notifier)
          .update(
            baseUrl: _urlController.text.trim(),
            token: _tokenController.text.trim(),
          );
      if (!mounted) return;
      ref.read(sessionsProvider.notifier).refresh();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Settings saved successfully'),
            backgroundColor: AppTheme.purpleAccent,
          ),
        );
        Navigator.pop(context);
      }
    } catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('$error')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text(
          'Settings',
          style: TextStyle(fontWeight: FontWeight.w700),
        ),
      ),
      body: ListView(
        padding: const EdgeInsets.all(20),
        children: [
          const Text(
            'Daemon Connection',
            style: TextStyle(fontWeight: FontWeight.w700, fontSize: 16),
          ),
          const SizedBox(height: 6),
          const Text(
            'Configure the address and authentication token for your OpenRemote daemon.',
            style: TextStyle(color: AppTheme.textMuted, fontSize: 13),
          ),
          const SizedBox(height: 16),
          const Text(
            'Server URL',
            style: TextStyle(
              color: AppTheme.textMuted,
              fontSize: 12,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 6),
          TextField(
            controller: _urlController,
            style: GoogleFonts.jetBrainsMono(fontSize: 13),
            decoration: const InputDecoration(
              hintText: 'http://127.0.0.1:4097',
              prefixIcon: Icon(Icons.link, size: 18),
            ),
          ),
          const SizedBox(height: 16),
          const Text(
            'Bearer Token',
            style: TextStyle(
              color: AppTheme.textMuted,
              fontSize: 12,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 6),
          TextField(
            controller: _tokenController,
            obscureText: true,
            style: GoogleFonts.jetBrainsMono(fontSize: 13),
            decoration: const InputDecoration(
              hintText: 'Paste 256-bit token from daemon output',
              prefixIcon: Icon(Icons.key, size: 18),
            ),
          ),
          const SizedBox(height: 16),
          Row(
            children: [
              OutlinedButton.icon(
                icon: _testingHealth
                    ? const SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.bolt, size: 16),
                label: const Text('Test Connection'),
                onPressed: _testingHealth ? null : _testConnection,
              ),
              const SizedBox(width: 12),
              if (_healthOk != null)
                Row(
                  children: [
                    Icon(
                      _healthOk! ? Icons.check_circle : Icons.error,
                      color: _healthOk!
                          ? AppTheme.successGreen
                          : AppTheme.dangerRed,
                      size: 18,
                    ),
                    const SizedBox(width: 6),
                    Text(
                      _healthOk! ? 'Connected' : 'Unreachable',
                      style: TextStyle(
                        color: _healthOk!
                            ? AppTheme.successGreen
                            : AppTheme.dangerRed,
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ],
                ),
            ],
          ),
          const SizedBox(height: 32),
          const Text(
            'Daemon',
            style: TextStyle(fontWeight: FontWeight.w700, fontSize: 16),
          ),
          const SizedBox(height: 8),
          _buildDaemonSection(),
          const SizedBox(height: 32),
          const Text(
            'Connecting from another device',
            style: TextStyle(fontWeight: FontWeight.w700, fontSize: 16),
          ),
          const SizedBox(height: 8),
          const Text(
            'Use the HTTPS address supplied by your tunnel. A localhost address refers to the device running this app. Keep your token private: it grants access to your sessions and allowed workspaces.',
            style: TextStyle(
              color: AppTheme.textMuted,
              fontSize: 13,
              height: 1.5,
            ),
          ),
          const SizedBox(height: 32),
          SizedBox(
            width: double.infinity,
            height: 44,
            child: ElevatedButton(
              onPressed: _save,
              child: const Text(
                'Save Settings',
                style: TextStyle(fontSize: 15),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildDaemonSection() {
    final statusAsync = ref.watch(updateStatusProvider);
    return statusAsync.maybeWhen(
      data: (status) {
        if (status == null) {
          return const Text(
            'Daemon version unknown (offline, or daemon predates update support).',
            style: TextStyle(color: AppTheme.textMuted, fontSize: 13, height: 1.5),
          );
        }
        final row = RichText(
          text: TextSpan(
            style: const TextStyle(color: AppTheme.textMuted, fontSize: 13, height: 1.5),
            children: [
              const TextSpan(text: 'Version '),
              TextSpan(
                text: status.current.isEmpty ? 'unknown' : status.current,
                style: const TextStyle(color: Colors.white, fontWeight: FontWeight.w600),
              ),
              if (status.available && (status.latest ?? '').isNotEmpty)
                TextSpan(text: '  ·  update available: ${status.latest}'),
              if (!status.available && (status.latest ?? '').isNotEmpty)
                TextSpan(text: '  ·  latest release ${status.latest}'),
              TextSpan(
                text: status.autoCheck == 'off'
                    ? '  ·  release checks off'
                    : '  ·  checks every ${status.autoCheck}',
              ),
            ],
          ),
        );
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            row,
            if (status.applyError != null && status.applyError!.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(
                  'Last update failed: ${status.applyError}',
                  style: const TextStyle(color: AppTheme.dangerRed, fontSize: 12),
                ),
              ),
            if (status.available)
              Padding(
                padding: const EdgeInsets.only(top: 12),
                child: OutlinedButton.icon(
                  icon: _applyingUpdate
                      ? const SizedBox(
                          width: 14,
                          height: 14,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.system_update, size: 16),
                  label: Text(_applyingUpdate ? 'Updating…' : 'Update now'),
                  onPressed: _applyingUpdate ? null : _applyDaemonUpdate,
                ),
              ),
          ],
        );
      },
      orElse: () => const Text(
        'Checking daemon status…',
        style: TextStyle(color: AppTheme.textMuted, fontSize: 13),
      ),
    );
  }
}
