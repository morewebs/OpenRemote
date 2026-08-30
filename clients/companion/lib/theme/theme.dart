import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';

class AppTheme {
  // Neutral Zinc Palette
  static const Color bgDark = Color(0xFF09090B);      // zinc-950
  static const Color surfaceDark = Color(0xFF18181B); // zinc-900
  static const Color cardDark = Color(0xFF27272A);    // zinc-800
  static const Color borderDark = Color(0xFF3F3F46);  // zinc-700
  static const Color textMain = Color(0xFFF4F4F5);    // zinc-100
  static const Color textMuted = Color(0xFFA1A1AA);   // zinc-400

  // Purple Accent
  static const Color purpleAccent = Color(0xFF7C3AED);     // violet-600
  static const Color purpleLight = Color(0xFF8B5CF6);      // violet-500
  static const Color purpleDark = Color(0xFF6D28D9);       // violet-700
  static const Color purpleGlow = Color(0x337C3AED);

  // Status Accents
  static const Color successGreen = Color(0xFF10B981);
  static const Color warningAmber = Color(0xFFF59E0B);
  static const Color dangerRed = Color(0xFFEF4444);

  static ThemeData get darkTheme {
    return ThemeData(
      useMaterial3: true,
      brightness: Brightness.dark,
      scaffoldBackgroundColor: bgDark,
      primaryColor: purpleAccent,
      colorScheme: const ColorScheme.dark(
        primary: purpleAccent,
        secondary: purpleLight,
        surface: surfaceDark,
        error: dangerRed,
        onPrimary: Colors.white,
        onSurface: textMain,
      ),
      textTheme: GoogleFonts.interTextTheme(
        ThemeData.dark().textTheme.copyWith(
          bodyLarge: const TextStyle(color: textMain, fontSize: 15, height: 1.5),
          bodyMedium: const TextStyle(color: textMain, fontSize: 14, height: 1.4),
          bodySmall: const TextStyle(color: textMuted, fontSize: 12),
          titleLarge: const TextStyle(color: textMain, fontSize: 18, fontWeight: FontWeight.w600),
          titleMedium: const TextStyle(color: textMain, fontSize: 15, fontWeight: FontWeight.w600),
        ),
      ),
      appBarTheme: const AppBarTheme(
        backgroundColor: bgDark,
        elevation: 0,
        scrolledUnderElevation: 0,
        centerTitle: false,
        titleTextStyle: TextStyle(
          color: textMain,
          fontSize: 16,
          fontWeight: FontWeight.w600,
        ),
        iconTheme: IconThemeData(color: textMain),
      ),
      cardTheme: CardThemeData(
        color: surfaceDark,
        elevation: 0,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(12),
          side: const BorderSide(color: borderDark, width: 1),
        ),
        margin: EdgeInsets.zero,
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: surfaceDark,
        hintStyle: const TextStyle(color: textMuted, fontSize: 14),
        contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: const BorderSide(color: borderDark, width: 1),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: const BorderSide(color: borderDark, width: 1),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: const BorderSide(color: purpleAccent, width: 1.5),
        ),
      ),
      elevatedButtonTheme: ElevatedButtonThemeData(
        style: ElevatedButton.styleFrom(
          backgroundColor: purpleAccent,
          foregroundColor: Colors.white,
          elevation: 0,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(8),
          ),
          textStyle: const TextStyle(
            fontSize: 14,
            fontWeight: FontWeight.w600,
          ),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          foregroundColor: textMain,
          side: const BorderSide(color: borderDark, width: 1),
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(8),
          ),
        ),
      ),
      dividerTheme: const DividerThemeData(
        color: borderDark,
        thickness: 1,
        space: 1,
      ),
    );
  }
}
