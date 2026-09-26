import 'dart:convert';
import 'package:http/http.dart' as http;
import '../models/models.dart';

class ApiException implements Exception {
  final int status;
  final String message;
  const ApiException(this.status, this.message);
  @override
  String toString() => message;
}

class ApiService {
  final String baseUrl;
  final String token;
  ApiService({String baseUrl = 'http://127.0.0.1:4097', this.token = ''})
    : baseUrl = baseUrl.replaceFirst(RegExp(r'/+$'), '');

  Future<http.Response> _request(
    String method,
    String path, {
    Object? body,
    Map<String, String>? query,
    Duration timeout = const Duration(seconds: 45),
  }) async {
    final uri = Uri.parse('$baseUrl$path').replace(queryParameters: query);
    final client = http.Client();
    try {
      final request = http.Request(method, uri)
        ..headers['Content-Type'] = 'application/json';
      if (token.isNotEmpty) request.headers['Authorization'] = 'Bearer $token';
      if (body != null) request.body = jsonEncode(body);
      final response = await client
          .send(request)
          .then(http.Response.fromStream)
          .timeout(timeout);
      if (response.statusCode < 200 || response.statusCode >= 300) {
        var message = response.body.trim();
        try {
          final error = jsonDecode(message);
          if (error is Map) {
            message =
                '${error['message'] ?? error['error'] ?? error['code'] ?? message}';
          }
        } catch (_) {}
        if (message.isEmpty) {
          message = 'Request failed (${response.statusCode}).';
        }
        if (response.statusCode == 401) {
          message = 'Connection needs a valid token. Update it in Settings.';
        }
        throw ApiException(
          response.statusCode,
          message.length > 500 ? '${message.substring(0, 500)}…' : message,
        );
      }
      return response;
    } finally {
      client.close();
    }
  }

  Future<bool> checkHealth() async {
    try {
      await _request('GET', '/health', timeout: const Duration(seconds: 3));
      return true;
    } catch (_) {
      return false;
    }
  }

  Future<List<AgentInfo>> getAgents() async {
    final response = await _request('GET', '/api/v1/agents');
    return (jsonDecode(response.body) as List? ?? [])
        .map((e) => AgentInfo.fromJson(e))
        .toList();
  }

  /// Daemon self-update status; null when the daemon predates the endpoint.
  Future<DaemonUpdateStatus?> getUpdateStatus() async {
    try {
      final response = await _request('GET', '/api/v1/update');
      return DaemonUpdateStatus.fromJson(jsonDecode(response.body));
    } on ApiException catch (e) {
      if (e.status == 404) return null;
      rethrow;
    }
  }

  /// Ask the daemon to apply its update (returns when accepted, not applied).
  Future<void> applyUpdate() async {
    await _request('POST', '/api/v1/update',
        timeout: const Duration(seconds: 10));
  }

  Future<List<SessionItem>> getSessions() async {
    final response = await _request('GET', '/api/v1/sessions');
    return (jsonDecode(response.body) as List? ?? [])
        .map((e) => SessionItem.fromJson(e))
        .toList();
  }

  Future<SessionItem> getSession(String id) async {
    final response = await _request(
      'GET',
      '/api/v1/sessions/${Uri.encodeComponent(id)}',
    );
    return SessionItem.fromJson(jsonDecode(response.body));
  }

  Future<SessionItem> createSession({
    required String agentId,
    required String cwd,
    bool useWorktree = false,
    String? taskName,
    int cols = 120,
    int rows = 30,
  }) async {
    final response = await _request(
      'POST',
      '/api/v1/sessions',
      body: {
        'agentId': agentId,
        'cwd': cwd,
        'useWorktree': useWorktree,
        if (taskName?.isNotEmpty == true) 'taskName': taskName,
        'cols': cols,
        'rows': rows,
      },
    );
    final result = jsonDecode(response.body) as Map<String, dynamic>;
    return getSession(result['sessionId'] as String);
  }

  Future<void> deleteSession(String id) async {
    await _request('DELETE', '/api/v1/sessions/${Uri.encodeComponent(id)}');
  }

  Future<void> sendPrompt(String id, String prompt) async {
    await _request(
      'POST',
      '/api/v1/sessions/${Uri.encodeComponent(id)}/prompt',
      body: {'prompt': prompt},
    );
  }

  Future<void> sendApproval(String id, bool approved) async {
    await _request(
      'POST',
      '/api/v1/approval/${Uri.encodeComponent(id)}',
      body: {'approved': approved},
    );
  }

  Future<void> sendAnswer(String id, List<dynamic> answers) async {
    await _request(
      'POST',
      '/api/v1/question/${Uri.encodeComponent(id)}',
      body: {'answers': answers},
    );
  }

  Future<String> getDiff(String id) async {
    return (await _request(
      'GET',
      '/api/v1/diff/${Uri.encodeComponent(id)}',
    )).body;
  }

  Future<List<Map<String, dynamic>>> getEvents(
    String id, {
    int since = 0,
  }) async {
    final response = await _request(
      'GET',
      '/api/v1/sessions/${Uri.encodeComponent(id)}',
      query: {'since': '$since'},
    );
    return (jsonDecode(response.body) as List? ?? [])
        .whereType<Map<String, dynamic>>()
        .toList();
  }

  Future<List<Map<String, dynamic>>> getFiles(String directory) async {
    final response = await _request(
      'GET',
      '/api/v1/files',
      query: {'dir': directory},
    );
    return (jsonDecode(response.body) as List? ?? [])
        .whereType<Map<String, dynamic>>()
        .toList();
  }

  Future<Map<String, dynamic>> getFile(String path) async {
    final response = await _request(
      'GET',
      '/api/v1/file',
      query: {'path': path},
    );
    return jsonDecode(response.body) as Map<String, dynamic>;
  }
}
