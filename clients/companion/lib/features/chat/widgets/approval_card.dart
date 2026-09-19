import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';
import '../../../models/models.dart';
import '../../../theme/theme.dart';

/// Displays a pending tool-execution approval request with Allow/Deny buttons.
/// The parent is responsible for wiring the callbacks to the API and provider.
class ApprovalCard extends StatelessWidget {
  final PendingApproval approval;
  final VoidCallback onAllow;
  final VoidCallback onDeny;

  const ApprovalCard({
    super.key,
    required this.approval,
    required this.onAllow,
    required this.onDeny,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.all(12),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: AppTheme.surfaceDark,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTheme.purpleAccent, width: 1.5),
        boxShadow: const [
          BoxShadow(
            color: AppTheme.purpleGlow,
            blurRadius: 12,
            spreadRadius: 1,
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(
                Icons.shield_outlined,
                color: AppTheme.purpleLight,
                size: 18,
              ),
              const SizedBox(width: 8),
              const Text(
                'Approval Requested',
                style: TextStyle(
                  fontWeight: FontWeight.w700,
                  fontSize: 14,
                  color: AppTheme.textMain,
                ),
              ),
              const Spacer(),
              Text(
                approval.toolName,
                style: GoogleFonts.jetBrainsMono(
                  fontSize: 11,
                  color: AppTheme.textMuted,
                ),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(10),
            decoration: BoxDecoration(
              color: AppTheme.bgDark,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: AppTheme.borderDark),
            ),
            child: Text(
              approval.command,
              style: GoogleFonts.jetBrainsMono(
                fontSize: 12,
                color: AppTheme.textMain,
              ),
            ),
          ),
          const SizedBox(height: 12),
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              OutlinedButton(
                onPressed: onDeny,
                child: const Text(
                  'Deny',
                  style: TextStyle(color: AppTheme.dangerRed),
                ),
              ),
              const SizedBox(width: 10),
              ElevatedButton.icon(
                icon: const Icon(Icons.check, size: 16),
                label: const Text('Allow'),
                onPressed: onAllow,
              ),
            ],
          ),
        ],
      ),
    );
  }
}
