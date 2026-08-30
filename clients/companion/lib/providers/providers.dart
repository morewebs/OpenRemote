import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../models/models.dart';
import '../services/api_service.dart';

class ServerConfig {
  final String baseUrl;
  final String token;

  const ServerConfig({
    this.baseUrl = 'http://127.0.0.1:4097',
    this.token = '',
  });

  ServerConfig copyWith({String? baseUrl, String? token}) {
    return ServerConfig(
      baseUrl: baseUrl ?? this.baseUrl,
      token: token ?? this.token,
    );
  }
}

class ServerConfigNotifier extends StateNotifier<ServerConfig> {
  ServerConfigNotifier() : super(const ServerConfig()) {
    _load();
  }

  Future<void> _load() async {
    final prefs = await SharedPreferences.getInstance();
    final url = prefs.getString('server_url') ?? 'http://127.0.0.1:4097';
    final tok = prefs.getString('server_token') ?? '';
    state = ServerConfig(baseUrl: url, token: tok);
  }

  Future<void> update({String? baseUrl, String? token}) async {
    final prefs = await SharedPreferences.getInstance();
    if (baseUrl != null) await prefs.setString('server_url', baseUrl);
    if (token != null) await prefs.setString('server_token', token);
    state = state.copyWith(baseUrl: baseUrl, token: token);
  }
}

final serverConfigProvider = StateNotifierProvider<ServerConfigNotifier, ServerConfig>((ref) {
  return ServerConfigNotifier();
});

final apiServiceProvider = Provider<ApiService>((ref) {
  final config = ref.watch(serverConfigProvider);
  return ApiService(baseUrl: config.baseUrl, token: config.token);
});

final wsConnectedProvider = StateProvider<bool>((ref) => false);

final agentsProvider = FutureProvider<List<AgentInfo>>((ref) async {
  final api = ref.watch(apiServiceProvider);
  try {
    return await api.getAgents();
  } catch (_) {
    return [];
  }
});

class SessionsNotifier extends StateNotifier<AsyncValue<List<SessionItem>>> {
  final ApiService _api;
  SessionsNotifier(this._api) : super(const AsyncValue.loading()) {
    refresh();
  }

  Future<void> refresh() async {
    try {
      final list = await _api.getSessions();
      state = AsyncValue.data(list);
    } catch (e, st) {
      state = AsyncValue.error(e, st);
    }
  }

  Future<SessionItem?> createSession({
    required String agentId,
    required String cwd,
    bool useWorktree = false,
    String? taskName,
  }) async {
    try {
      final s = await _api.createSession(
        agentId: agentId,
        cwd: cwd,
        useWorktree: useWorktree,
        taskName: taskName,
      );
      await refresh();
      return s;
    } catch (e) {
      return null;
    }
  }

  Future<void> deleteSession(String sessionId) async {
    try {
      await _api.deleteSession(sessionId);
      await refresh();
    } catch (_) {}
  }
}

final sessionsProvider = StateNotifierProvider<SessionsNotifier, AsyncValue<List<SessionItem>>>((ref) {
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
      if (msg.rev >= current.rev || !msg.streaming) {
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

final chatMessagesProvider = StateNotifierProvider<ChatNotifier, List<ChatMessage>>((ref) {
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

final pendingApprovalsProvider = StateNotifierProvider<ApprovalNotifier, List<PendingApproval>>((ref) {
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

final pendingQuestionsProvider = StateNotifierProvider<QuestionNotifier, List<PendingQuestion>>((ref) {
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

final authUrlCardsProvider = StateNotifierProvider<AuthUrlNotifier, List<AuthUrlCard>>((ref) {
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

final diffCardsProvider = StateNotifierProvider<DiffCardNotifier, List<DiffCard>>((ref) {
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

final turnSummariesProvider = StateNotifierProvider<TurnSummaryNotifier, List<TurnSummary>>((ref) {
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

final artifactCardsProvider = StateNotifierProvider<ArtifactCardNotifier, List<ArtifactCard>>((ref) {
  return ArtifactCardNotifier();
});
