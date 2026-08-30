import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:google_fonts/google_fonts.dart';
import '../../providers/providers.dart';
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

  void _testConnection() async {
    setState(() {
      _testingHealth = true;
      _healthOk = null;
    });
    final api = ref.read(apiServiceProvider);
    api.baseUrl = _urlController.text.trim();
    api.token = _tokenController.text.trim();
    final ok = await api.checkHealth();
    setState(() {
      _testingHealth = false;
      _healthOk = ok;
    });
  }

  void _save() async {
    await ref.read(serverConfigProvider.notifier).update(
      baseUrl: _urlController.text.trim(),
      token: _tokenController.text.trim(),
    );
    ref.read(sessionsProvider.notifier).refresh();
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Settings saved successfully'), backgroundColor: AppTheme.purpleAccent),
      );
      Navigator.pop(context);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Settings', style: TextStyle(fontWeight: FontWeight.w700)),
      ),
      body: ListView(
        padding: const EdgeInsets.all(20),
        children: [
          const Text('Daemon Connection', style: TextStyle(fontWeight: FontWeight.w700, fontSize: 16)),
          const SizedBox(height: 6),
          const Text('Configure the address and authentication token for your OpenRemote daemon.', style: TextStyle(color: AppTheme.textMuted, fontSize: 13)),
          const SizedBox(height: 16),
          const Text('Server URL', style: TextStyle(color: AppTheme.textMuted, fontSize: 12, fontWeight: FontWeight.w600)),
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
          const Text('Bearer Token', style: TextStyle(color: AppTheme.textMuted, fontSize: 12, fontWeight: FontWeight.w600)),
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
                    ? const SizedBox(width: 14, height: 14, child: CircularProgressIndicator(strokeWidth: 2))
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
                      color: _healthOk! ? AppTheme.successGreen : AppTheme.dangerRed,
                      size: 18,
                    ),
                    const SizedBox(width: 6),
                    Text(
                      _healthOk! ? 'Connected' : 'Unreachable',
                      style: TextStyle(
                        color: _healthOk! ? AppTheme.successGreen : AppTheme.dangerRed,
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ],
                ),
            ],
          ),
          const SizedBox(height: 32),
          const Text('Architecture & Protocol', style: TextStyle(fontWeight: FontWeight.w700, fontSize: 16)),
          const SizedBox(height: 12),
          Container(
            padding: const EdgeInsets.all(14),
            decoration: BoxDecoration(
              color: AppTheme.surfaceDark,
              borderRadius: BorderRadius.circular(10),
              border: Border.all(color: AppTheme.borderDark),
            ),
            child: Column(
              children: [
                _buildInfoRow('Daemon Engine', 'Go + SQLite WAL'),
                const Divider(height: 16),
                _buildInfoRow('Framing', '2-byte binary WebSocket'),
                const Divider(height: 16),
                _buildInfoRow('Terminal', 'xterm.dart 4.0.0 (ConPTY)'),
                const Divider(height: 16),
                _buildInfoRow('Color Palette', 'Zinc Neutrals + Purple'),
              ],
            ),
          ),
          const SizedBox(height: 32),
          SizedBox(
            width: double.infinity,
            height: 44,
            child: ElevatedButton(
              onPressed: _save,
              child: const Text('Save Settings', style: TextStyle(fontSize: 15)),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildInfoRow(String label, String value) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      children: [
        Text(label, style: const TextStyle(color: AppTheme.textMuted, fontSize: 13)),
        Text(value, style: GoogleFonts.jetBrainsMono(fontSize: 12, color: AppTheme.textMain, fontWeight: FontWeight.w600)),
      ],
    );
  }
}
