class SessionItem {
  final String sessionId;
  final String workspaceId;
  final String agentId;
  final String cwd;
  final String? worktreePath;
  final String? branchName;
  final String status;
  final int createdAt;

  SessionItem({
    required this.sessionId,
    required this.workspaceId,
    required this.agentId,
    required this.cwd,
    this.worktreePath,
    this.branchName,
    required this.status,
    required this.createdAt,
  });

  factory SessionItem.fromJson(Map<String, dynamic> json) {
    return SessionItem(
      sessionId: json['sessionId'] as String? ?? '',
      workspaceId: json['workspaceId'] as String? ?? '',
      agentId: json['agentId'] as String? ?? '',
      cwd: json['cwd'] as String? ?? '',
      worktreePath: json['worktreePath'] as String?,
      branchName: json['branchName'] as String?,
      status: json['status'] as String? ?? 'stopped',
      createdAt: (json['createdAt'] as num?)?.toInt() ?? 0,
    );
  }

  bool get isRunning => status == 'running';
  String get shortId => sessionId.length > 8 ? sessionId.substring(0, 8) : sessionId;
}

class ChatMessage {
  final String id;
  final String sessionId;
  final String role; // 'user', 'assistant', 'tool', 'system'
  final String kind; // 'text', 'tool_use', 'tool_result', 'thought'
  final String text;
  final String? toolName;
  final Map<String, dynamic>? meta;
  final bool streaming;
  final int rev;
  final int timestamp;
  final int? seq;

  ChatMessage({
    required this.id,
    required this.sessionId,
    required this.role,
    required this.kind,
    required this.text,
    this.toolName,
    this.meta,
    required this.streaming,
    required this.rev,
    required this.timestamp,
    this.seq,
  });

  factory ChatMessage.fromJson(Map<String, dynamic> json) {
    return ChatMessage(
      id: json['messageId'] as String? ?? json['id'] as String? ?? '',
      sessionId: json['sessionId'] as String? ?? '',
      role: json['role'] as String? ?? 'assistant',
      kind: json['kind'] as String? ?? 'text',
      text: json['text'] as String? ?? '',
      toolName: json['toolName'] as String?,
      meta: json['meta'] as Map<String, dynamic>?,
      streaming: json['streaming'] as bool? ?? false,
      rev: (json['rev'] as num?)?.toInt() ?? 1,
      timestamp: (json['timestamp'] as num?)?.toInt() ?? DateTime.now().millisecondsSinceEpoch,
      seq: (json['seq'] as num?)?.toInt(),
    );
  }

  ChatMessage copyWith({
    String? text,
    bool? streaming,
    int? rev,
  }) {
    return ChatMessage(
      id: id,
      sessionId: sessionId,
      role: role,
      kind: kind,
      text: text ?? this.text,
      toolName: toolName,
      meta: meta,
      streaming: streaming ?? this.streaming,
      rev: rev ?? this.rev,
      timestamp: timestamp,
      seq: seq,
    );
  }

  bool get isUser => role == 'user';
  bool get isAssistant => role == 'assistant';
  bool get isTool => role == 'tool';
}

class PendingApproval {
  final String id;
  final String sessionId;
  final String toolName;
  final String command;
  final String? description;
  final int autoDenyTimeoutMs;
  bool resolved;
  bool approved;

  PendingApproval({
    required this.id,
    required this.sessionId,
    required this.toolName,
    required this.command,
    this.description,
    this.autoDenyTimeoutMs = 120000,
    this.resolved = false,
    this.approved = false,
  });

  factory PendingApproval.fromJson(Map<String, dynamic> json) {
    return PendingApproval(
      id: json['approvalId'] as String? ?? json['id'] as String? ?? '',
      sessionId: json['sessionId'] as String? ?? '',
      toolName: json['toolName'] as String? ?? 'Command',
      command: json['command'] as String? ?? '',
      description: json['description'] as String?,
      autoDenyTimeoutMs: (json['autoDenyTimeoutMs'] as num?)?.toInt() ?? 120000,
    );
  }
}

class PendingQuestion {
  final String id;
  final String sessionId;
  final String questionText;
  final List<String> options;
  final bool isMultiSelect;
  bool resolved;

  PendingQuestion({
    required this.id,
    required this.sessionId,
    required this.questionText,
    required this.options,
    this.isMultiSelect = false,
    this.resolved = false,
  });

  factory PendingQuestion.fromJson(Map<String, dynamic> json) {
    return PendingQuestion(
      id: json['questionId'] as String? ?? json['id'] as String? ?? '',
      sessionId: json['sessionId'] as String? ?? '',
      questionText: json['questionText'] as String? ?? json['question'] as String? ?? '',
      options: (json['options'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      isMultiSelect: json['isMultiSelect'] as bool? ?? false,
    );
  }
}

class AuthUrlCard {
  final String sessionId;
  final String url;
  final int timestamp;

  AuthUrlCard({required this.sessionId, required this.url, required this.timestamp});

  factory AuthUrlCard.fromJson(Map<String, dynamic> json) {
    return AuthUrlCard(
      sessionId: json['sessionId'] as String? ?? '',
      url: json['url'] as String? ?? '',
      timestamp: (json['timestamp'] as num?)?.toInt() ?? 0,
    );
  }
}

class AgentInfo {
  final String id;
  final String displayName;
  final bool available;
  final String? reason;
  final bool supportsTerminal;
  final bool supportsChatNative;
  final bool supportsApproval;
  final bool supportsDiff;

  AgentInfo({
    required this.id,
    required this.displayName,
    required this.available,
    this.reason,
    required this.supportsTerminal,
    required this.supportsChatNative,
    required this.supportsApproval,
    required this.supportsDiff,
  });

  factory AgentInfo.fromJson(Map<String, dynamic> json) {
    final caps = json['capabilities'] as Map<String, dynamic>? ?? {};
    return AgentInfo(
      id: json['id'] as String? ?? '',
      displayName: json['displayName'] as String? ?? json['id'] as String? ?? '',
      available: json['available'] as bool? ?? false,
      reason: json['reason'] as String?,
      supportsTerminal: caps['supportsTerminal'] as bool? ?? true,
      supportsChatNative: caps['supportsChatNative'] as bool? ?? true,
      supportsApproval: caps['supportsApproval'] as bool? ?? true,
      supportsDiff: caps['supportsDiff'] as bool? ?? true,
    );
  }
}

/// A unified git diff emitted by the daemon's stream parser (`diff.generated`).
class DiffCard {
  final String sessionId;
  final String filePath;
  final String diffPatch;
  final int additions;
  final int deletions;
  final int timestamp;

  DiffCard({
    required this.sessionId,
    required this.filePath,
    required this.diffPatch,
    required this.additions,
    required this.deletions,
    required this.timestamp,
  });

  factory DiffCard.fromJson(Map<String, dynamic> json) {
    return DiffCard(
      sessionId: json['sessionId'] as String? ?? '',
      filePath: json['filePath'] as String? ?? '',
      diffPatch: json['diffPatch'] as String? ?? '',
      additions: (json['additions'] as num?)?.toInt() ?? 0,
      deletions: (json['deletions'] as num?)?.toInt() ?? 0,
      timestamp: (json['timestamp'] as num?)?.toInt() ?? 0,
    );
  }
}

/// End-of-turn summary (`turn.completed`).
class TurnSummary {
  final String sessionId;
  final String? summary;
  final double? costUsd;
  final int durationMs;

  TurnSummary({
    required this.sessionId,
    this.summary,
    this.costUsd,
    required this.durationMs,
  });

  factory TurnSummary.fromJson(Map<String, dynamic> json) {
    return TurnSummary(
      sessionId: json['sessionId'] as String? ?? '',
      summary: json['summary'] as String?,
      costUsd: (json['costUsd'] as num?)?.toDouble(),
      durationMs: (json['durationMs'] as num?)?.toInt() ?? 0,
    );
  }
}

/// A plan/diff/file artifact written by the agent (`artifact.updated`).
class ArtifactCard {
  final String sessionId;
  final String path;
  final String kind;
  final String content;

  ArtifactCard({
    required this.sessionId,
    required this.path,
    required this.kind,
    required this.content,
  });

  factory ArtifactCard.fromJson(Map<String, dynamic> json) {
    return ArtifactCard(
      sessionId: json['sessionId'] as String? ?? '',
      path: json['path'] as String? ?? '',
      kind: json['kind'] as String? ?? 'file',
      content: json['content'] as String? ?? '',
    );
  }
}
