import 'dart:convert';
import 'package:http/http.dart' as http;
import '../models/models.dart';

class ApiService {
  String baseUrl;
  String token;

  ApiService({
    this.baseUrl = 'http://127.0.0.1:4097',
    this.token = '',
  });

  Map<String, String> get _headers {
    final headers = {
      'Content-Type': 'application/json',
    };
    if (token.isNotEmpty) {
      headers['Authorization'] = 'Bearer $token';
    }
    return headers;
  }

  Future<bool> checkHealth() async {
    try {
      final res = await http.get(Uri.parse('$baseUrl/health')).timeout(const Duration(seconds: 3));
      return res.statusCode == 200;
    } catch (_) {
      return false;
    }
  }

  Future<List<AgentInfo>> getAgents() async {
    final res = await http.get(Uri.parse('$baseUrl/api/v1/agents'), headers: _headers);
    if (res.statusCode == 200) {
      final List data = jsonDecode(res.body);
      return data.map((e) => AgentInfo.fromJson(e)).toList();
    }
    throw Exception('Failed to load agents: ${res.statusCode}');
  }

  Future<List<SessionItem>> getSessions() async {
    final res = await http.get(Uri.parse('$baseUrl/api/v1/sessions'), headers: _headers);
    if (res.statusCode == 200) {
      final List data = jsonDecode(res.body);
      return data.map((e) => SessionItem.fromJson(e)).toList();
    }
    throw Exception('Failed to load sessions: ${res.statusCode}');
  }

  Future<SessionItem> createSession({
    required String agentId,
    required String cwd,
    bool useWorktree = false,
    String? taskName,
    int cols = 120,
    int rows = 30,
  }) async {
    final body = jsonEncode({
      'agentId': agentId,
      'cwd': cwd,
      'useWorktree': useWorktree,
      if (taskName != null && taskName.isNotEmpty) 'taskName': taskName,
      'cols': cols,
      'rows': rows,
    });

    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/sessions'),
      headers: _headers,
      body: body,
    );

    if (res.statusCode == 201) {
      final data = jsonDecode(res.body);
      return SessionItem(
        sessionId: data['sessionId'],
        workspaceId: data['workspaceId'] ?? '',
        agentId: agentId,
        cwd: cwd,
        worktreePath: data['worktreePath'],
        status: data['status'] ?? 'running',
        createdAt: DateTime.now().millisecondsSinceEpoch,
      );
    }
    throw Exception('Create session failed (${res.statusCode}): ${res.body}');
  }

  Future<void> deleteSession(String sessionId) async {
    final res = await http.delete(Uri.parse('$baseUrl/api/v1/sessions/$sessionId'), headers: _headers);
    if (res.statusCode != 204 && res.statusCode != 200) {
      throw Exception('Delete session failed: ${res.statusCode}');
    }
  }

  Future<void> sendPrompt(String sessionId, String prompt) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/sessions/$sessionId/prompt'),
      headers: _headers,
      body: jsonEncode({'prompt': prompt}),
    );
    if (res.statusCode != 200) {
      throw Exception('Send prompt failed: ${res.statusCode}');
    }
  }

  Future<void> sendApproval(String approvalId, bool approved) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/approval/$approvalId'),
      headers: _headers,
      body: jsonEncode({'approved': approved}),
    );
    if (res.statusCode != 200) {
      throw Exception('Resolve approval failed: ${res.statusCode}');
    }
  }

  Future<String> getDiff(String sessionId) async {
    final res = await http.get(Uri.parse('$baseUrl/api/v1/diff/$sessionId'), headers: _headers);
    if (res.statusCode == 200) {
      return res.body;
    }
    return '';
  }

  Future<void> sendAnswer(String questionId, List<dynamic> answers) async {
    final res = await http.post(
      Uri.parse('$baseUrl/api/v1/question/$questionId'),
      headers: _headers,
      body: jsonEncode({'answers': answers}),
    );
    if (res.statusCode != 200) {
      throw Exception('Answer question failed: ${res.statusCode}');
    }
  }

  /// Fetches persisted session events after [since] (monotonic seq), used to
  /// hydrate the chat transcript on first open and after reconnects.
  Future<List<Map<String, dynamic>>> getEvents(String sessionId, {int since = 0}) async {
    final res = await http
        .get(Uri.parse('$baseUrl/api/v1/sessions/$sessionId?since=$since'), headers: _headers);
    if (res.statusCode == 200) {
      final List data = jsonDecode(res.body);
      return data.whereType<Map<String, dynamic>>().toList();
    }
    return [];
  }
}
