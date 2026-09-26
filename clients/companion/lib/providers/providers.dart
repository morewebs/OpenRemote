import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../models/models.dart';
import '../services/api_service.dart';

class ServerConfig {
  final String baseUrl;
  final String token;

  const ServerConfig({this.baseUrl = 'http://127.0.0.1:4097', this.token = ''});

  ServerConfig copyWith({String? baseUrl, String? token}) {
    return ServerConfig(
      baseUrl: baseUrl ?? this.baseUrl,
      token: token ?? this.token,
    );
  }
}

String get defaultServerUrl =>
    kIsWeb && (Uri.base.scheme == 'http' || Uri.base.scheme == 'https')
    ? Uri.base.origin
    : 'http://127.0.0.1:4097';

class ServerConfigNotifier extends StateNotifier<ServerConfig> {
  ServerConfigNotifier() : super(ServerConfig(baseUrl: defaultServerUrl)) {
    _load();
  }

  Future<void> _load() async {
    final prefs = await SharedPreferences.getInstance();
    final url = prefs.getString('server_url') ?? defaultServerUrl;
    final tok = prefs.getString('server_token') ?? '';
    state = ServerConfig(baseUrl: url, token: tok);
  }

  Future<void> update({String? baseUrl, String? token}) async {
    if (baseUrl != null) {
      final uri = Uri.tryParse(baseUrl.trim());
      if (uri == null ||
          !['http', 'https'].contains(uri.scheme) ||
          uri.host.isEmpty ||
          uri.userInfo.isNotEmpty ||
          uri.hasQuery ||
          uri.hasFragment) {
        throw const FormatException(
          'Enter an http:// or https:// daemon address.',
        );
      }
      baseUrl = baseUrl.trim().replaceFirst(RegExp(r'/+$'), '');
    }
    final prefs = await SharedPreferences.getInstance();
    if (baseUrl != null) await prefs.setString('server_url', baseUrl);
    if (token != null) await prefs.setString('server_token', token);
    state = state.copyWith(baseUrl: baseUrl, token: token);
  }
}

final serverConfigProvider =
    StateNotifierProvider<ServerConfigNotifier, ServerConfig>((ref) {
      return ServerConfigNotifier();
    });

final apiServiceProvider = Provider<ApiService>((ref) {
  final config = ref.watch(serverConfigProvider);
  return ApiService(baseUrl: config.baseUrl, token: config.token);
});

final wsConnectedProvider = StateProvider<bool>((ref) => false);

final agentsProvider = FutureProvider<List<AgentInfo>>((ref) async {
  final api = ref.watch(apiServiceProvider);
  return api.getAgents();
});

/// Daemon self-update status; null when the daemon is offline or predates
/// the update endpoint (older daemons answer 404).
final updateStatusProvider = FutureProvider<DaemonUpdateStatus?>((ref) async {
  final api = ref.watch(apiServiceProvider);
  try {
    return await api.getUpdateStatus();
  } catch (_) {
    return null;
  }
});

/// Remembers the latest release version whose banner was dismissed, so the
/// banner only reappears for a genuinely new release.
class DismissedUpdateNotifier extends StateNotifier<String?> {
  DismissedUpdateNotifier() : super(null) {
    _load();
  }

  Future<void> _load() async {
    final prefs = await SharedPreferences.getInstance();
    state = prefs.getString('dismissed_update_version');
  }

  Future<void> dismiss(String version) async {
    state = version;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('dismissed_update_version', version);
  }
}

final dismissedUpdateProvider =
    StateNotifierProvider<DismissedUpdateNotifier, String?>((ref) {
  return DismissedUpdateNotifier();
});

class SessionsNotifier extends StateNotifier<AsyncValue<List<SessionItem>>> {
  final ApiService _api;
  SessionsNotifier(this._api) : super(const AsyncValue.loading()) {
    refresh();
  }

  Future<void> refresh() async {
    try {
      final list = await _api.getSessions();
      if (mounted) state = AsyncValue.data(list);
    } catch (e, st) {
      if (mounted) state = AsyncValue.error(e, st);
    }
  }

  Future<SessionItem?> createSession({
    required String agentId,
    required String cwd,
    bool useWorktree = false,
    String? taskName,
  }) async {
    final s = await _api.createSession(
      agentId: agentId,
      cwd: cwd,
      useWorktree: useWorktree,
      taskName: taskName,
    );
    await refresh();
    return s;
  }

  Future<void> deleteSession(String sessionId) async {
    await _api.deleteSession(sessionId);
    await refresh();
  }
}

final sessionsProvider =
    StateNotifierProvider<SessionsNotifier, AsyncValue<List<SessionItem>>>((
      ref,
    ) {
      final api = ref.watch(apiServiceProvider);
      return SessionsNotifier(api);
    });

final activeSessionIdProvider = StateProvider<String?>((ref) => null);

final activeSessionProvider = Provider<SessionItem?>((ref) {
  final id = ref.watch(activeSessionIdProvider);
  final sessionsAsync = ref.watch(sessionsProvider);
  if (id == null) return null;
  return sessionsAsync.valueOrNull?.firstWhere(
    (s) => s.sessionId == id,
    orElse: () => SessionItem(
      sessionId: id,
      workspaceId: '',
      agentId: '',
      cwd: '',
      status: 'running',
      createdAt: 0,
    ),
  );
});

class ChatNotifier extends StateNotifier<List<ChatMessage>> {
  ChatNotifier() : super([]);

  void addOrUpdateMessage(ChatMessage msg) {
    final idx = state.indexWhere((m) => m.id == msg.id);
    if (idx >= 0) {
      final current = state[idx];
      if (msg.rev >= current.rev) {
        final updated = List<ChatMessage>.from(state);
        updated[idx] = msg;
        state = updated;
      }
    } else {
      state = [...state, msg];
    }
  }

  void appendUserMessage(String sessionId, String text) {
    final msg = ChatMessage(
      id: 'usr_${DateTime.now().millisecondsSinceEpoch}',
      sessionId: sessionId,
      role: 'user',
      kind: 'text',
      text: text,
      streaming: false,
      rev: 1,
      timestamp: DateTime.now().millisecondsSinceEpoch,
    );
    state = [...state, msg];
  }

  void clear() {
    state = [];
  }
}

/// Session-scoped chat transcript. Keyed by `sessionId` and disposed
/// automatically when the owning screen stops listening, so opening a second
/// session never leaks the first session's messages into the transcript.
final chatMessagesProvider = StateNotifierProvider.autoDispose
    .family<ChatNotifier, List<ChatMessage>, String>((ref, sessionId) {
      return ChatNotifier();
    });

class ApprovalNotifier extends StateNotifier<List<PendingApproval>> {
  ApprovalNotifier() : super([]);

  void addApproval(PendingApproval app) {
    if (!state.any((a) => a.id == app.id)) {
      state = [...state, app];
    }
  }

  void resolve(String id, bool approved) {
    state = state.map((a) {
      if (a.id == id) {
        a.resolved = true;
        a.approved = approved;
      }
      return a;
    }).toList();
  }

  void clear() {
    state = [];
  }
}

/// Pending approvals for a single session. Scoped to avoid leaking
/// approval state across sessions when the global singleton was used.
final pendingApprovalsProvider = StateNotifierProvider.autoDispose
    .family<ApprovalNotifier, List<PendingApproval>, String>((ref, sessionId) {
      return ApprovalNotifier();
    });

class QuestionNotifier extends StateNotifier<List<PendingQuestion>> {
  QuestionNotifier() : super([]);

  void addQuestion(PendingQuestion q) {
    if (!state.any((x) => x.id == q.id)) {
      state = [...state, q];
    }
  }

  void resolve(String id) {
    state = state.map((q) {
      if (q.id == id) q.resolved = true;
      return q;
    }).toList();
  }

  void clear() {
    state = [];
  }
}

/// Questions/disambiguations scoped per session. See family note above.
final pendingQuestionsProvider = StateNotifierProvider.autoDispose
    .family<QuestionNotifier, List<PendingQuestion>, String>((ref, sessionId) {
      return QuestionNotifier();
    });

class AuthUrlNotifier extends StateNotifier<List<AuthUrlCard>> {
  AuthUrlNotifier() : super([]);

  void addCard(AuthUrlCard card) {
    if (!state.any((c) => c.url == card.url)) {
      state = [...state, card];
    }
  }

  void dismiss(String url) {
    state = state.where((c) => c.url != url).toList();
  }

  void clear() {
    state = [];
  }
}

/// Auth/OAuth URL cards shown inline in chat, scoped per session so
/// other sessions do not inherit the viewing session's login prompts.
final authUrlCardsProvider = StateNotifierProvider.autoDispose
    .family<AuthUrlNotifier, List<AuthUrlCard>, String>((ref, sessionId) {
      return AuthUrlNotifier();
    });

class DiffCardNotifier extends StateNotifier<List<DiffCard>> {
  DiffCardNotifier() : super([]);

  void addCard(DiffCard card) {
    state = [...state, card];
  }

  void clear() {
    state = [];
  }
}

/// Unified git diffs emitted live (`diff.generated`), scoped per session.
final diffCardsProvider = StateNotifierProvider.autoDispose
    .family<DiffCardNotifier, List<DiffCard>, String>((ref, sessionId) {
      return DiffCardNotifier();
    });

class TurnSummaryNotifier extends StateNotifier<List<TurnSummary>> {
  TurnSummaryNotifier() : super([]);

  void addSummary(TurnSummary summary) {
    state = [...state, summary];
  }

  void clear() {
    state = [];
  }
}

/// End-of-turn summaries (`turn.completed`), per session.
final turnSummariesProvider = StateNotifierProvider.autoDispose
    .family<TurnSummaryNotifier, List<TurnSummary>, String>((ref, sessionId) {
      return TurnSummaryNotifier();
    });

class ArtifactCardNotifier extends StateNotifier<List<ArtifactCard>> {
  ArtifactCardNotifier() : super([]);

  void addOrUpdateArtifact(ArtifactCard card) {
    final idx = state.indexWhere((a) => a.path == card.path);
    if (idx >= 0) {
      final updated = List<ArtifactCard>.from(state);
      updated[idx] = card;
      state = updated;
    } else {
      state = [...state, card];
    }
  }

  void clear() {
    state = [];
  }
}

/// Artifact cards (plan/diff/file) scoped per session.
final artifactCardsProvider = StateNotifierProvider.autoDispose
    .family<ArtifactCardNotifier, List<ArtifactCard>, String>((ref, sessionId) {
      return ArtifactCardNotifier();
    });
