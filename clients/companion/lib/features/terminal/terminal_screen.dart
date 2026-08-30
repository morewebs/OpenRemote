import 'dart:convert';
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:google_fonts/google_fonts.dart';
import 'package:xterm/xterm.dart';
import '../../providers/providers.dart';
import '../../services/ws_service.dart';
import '../../theme/theme.dart';

class TerminalScreen extends ConsumerStatefulWidget {
  final String sessionId;
  const TerminalScreen({super.key, required this.sessionId});

  @override
  ConsumerState<TerminalScreen> createState() => _TerminalScreenState();
}

class _TerminalScreenState extends ConsumerState<TerminalScreen> {
  late final Terminal _terminal;
  WebSocketService? _ws;

  @override
  void initState() {
    super.initState();
    _terminal = Terminal(maxLines: 5000);

    _terminal.onOutput = (data) {
      final bytes = Uint8List.fromList(utf8.encode(data));
      _ws?.sendKeystroke(bytes);
    };

    _terminal.onResize = (w, h, pw, ph) {
      _ws?.sendResize(w, h);
    };

    Future.microtask(_initWebSocket);
  }

  void _initWebSocket() {
    final config = ref.read(serverConfigProvider);
    final wsBase = config.baseUrl.replaceFirst('http', 'ws');
    final wsUrl = '$wsBase/ws';

    _ws = WebSocketService(
      onPtyOutput: (bytes) {
        final text = utf8.decode(bytes, allowMalformed: true);
        _terminal.write(text);
      },
      onConnectionChange: (connected) {
        ref.read(wsConnectedProvider.notifier).state = connected;
      },
    );

    _ws?.connect(wsUrl, sessionId: widget.sessionId, token: config.token);
  }

  @override
  void dispose() {
    _ws?.disconnect();
    super.dispose();
  }

  void _sendSpecialKey(String seq) {
    final bytes = Uint8List.fromList(utf8.encode(seq));
    _ws?.sendKeystroke(bytes);
  }

  @override
  Widget build(BuildContext context) {
    final isConnected = ref.watch(wsConnectedProvider);

    return Scaffold(
      appBar: AppBar(
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
            const Text('Terminal Escape Hatch', style: TextStyle(fontSize: 15, fontWeight: FontWeight.w700)),
          ],
        ),
        actions: [
          IconButton(
            tooltip: 'Clear Screen',
            icon: const Icon(Icons.clear_all, size: 20),
            onPressed: () {
              _terminal.eraseDisplay();
            },
          ),
        ],
      ),
      body: Column(
        children: [
          Expanded(
            child: Container(
              color: Colors.black,
              padding: const EdgeInsets.all(4),
              child: TerminalView(
                _terminal,
                textStyle: TerminalStyle(
                  fontSize: 13,
                  fontFamily: GoogleFonts.jetBrainsMono().fontFamily ?? 'monospace',
                ),
                theme: const TerminalTheme(
                  cursor: AppTheme.purpleAccent,
                  selection: AppTheme.purpleGlow,
                  foreground: Color(0xFFE4E4E7),
                  background: Colors.black,
                  black: Color(0xFF27272A),
                  red: Color(0xFFEF4444),
                  green: Color(0xFF10B981),
                  yellow: Color(0xFFF59E0B),
                  blue: Color(0xFF3B82F6),
                  magenta: Color(0xFF8B5CF6),
                  cyan: Color(0xFF06B6D4),
                  white: Color(0xFFF4F4F5),
                  brightBlack: Color(0xFF52525B),
                  brightRed: Color(0xFFF87171),
                  brightGreen: Color(0xFF34D399),
                  brightYellow: Color(0xFFFBBF24),
                  brightBlue: Color(0xFF60A5FA),
                  brightMagenta: Color(0xFFA78BFA),
                  brightCyan: Color(0xFF22D3EE),
                  brightWhite: Color(0xFFFFFFFF),
                  searchHitBackground: Color(0xFFFBBF24),
                  searchHitBackgroundCurrent: Color(0xFFF59E0B),
                  searchHitForeground: Colors.black,
                ),
              ),
            ),
          ),
          // Mobile Accessory Keys
          _buildAccessoryBar(),
        ],
      ),
    );
  }

  Widget _buildAccessoryBar() {
    return Container(
      color: AppTheme.surfaceDark,
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      child: SafeArea(
        child: SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: Row(
            children: [
              _buildKeyButton('ESC', '\x1b'),
              _buildKeyButton('TAB', '\t'),
              _buildKeyButton('^C', '\x03'),
              _buildKeyButton('^D', '\x04'),
              _buildKeyButton('^Z', '\x1a'),
              _buildKeyButton('▲', '\x1b[A'),
              _buildKeyButton('▼', '\x1b[B'),
              _buildKeyButton('◀', '\x1b[D'),
              _buildKeyButton('▶', '\x1b[C'),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildKeyButton(String label, String seq) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 4),
      child: InkWell(
        onTap: () => _sendSpecialKey(seq),
        borderRadius: BorderRadius.circular(4),
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
          decoration: BoxDecoration(
            color: AppTheme.cardDark,
            borderRadius: BorderRadius.circular(4),
            border: Border.all(color: AppTheme.borderDark),
          ),
          child: Text(
            label,
            style: GoogleFonts.jetBrainsMono(fontSize: 12, fontWeight: FontWeight.w600, color: AppTheme.textMain),
          ),
        ),
      ),
    );
  }
}
