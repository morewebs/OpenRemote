import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'features/chat/chat_screen.dart';
import 'features/sessions/sessions_screen.dart';
import 'features/settings/settings_screen.dart';
import 'features/terminal/terminal_screen.dart';
import 'theme/theme.dart';

final routerProvider = Provider<GoRouter>((ref) {
  return GoRouter(
    initialLocation: '/',
    routes: [
      GoRoute(
        path: '/',
        builder: (context, state) => const SessionsScreen(),
      ),
      GoRoute(
        path: '/session/:id',
        builder: (context, state) {
          final id = state.pathParameters['id'] ?? '';
          return ChatScreen(sessionId: id);
        },
      ),
      GoRoute(
        path: '/session/:id/terminal',
        builder: (context, state) {
          final id = state.pathParameters['id'] ?? '';
          return TerminalScreen(sessionId: id);
        },
      ),
      GoRoute(
        path: '/settings',
        builder: (context, state) => const SettingsScreen(),
      ),
    ],
  );
});

class OpenRemoteApp extends ConsumerWidget {
  const OpenRemoteApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final router = ref.watch(routerProvider);

    return MaterialApp.router(
      title: 'OpenRemote',
      debugShowCheckedModeBanner: false,
      theme: AppTheme.darkTheme,
      routerConfig: router,
    );
  }
}
