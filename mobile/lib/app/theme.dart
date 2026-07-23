import 'package:flutter/material.dart';

/// Centralized theming for DeskSync. A single seed color drives both light and
/// dark Material 3 color schemes so the app stays visually consistent.
class DeskSyncTheme {
  const DeskSyncTheme._();

  static const Color _seed = Color(0xFF3B6EF5);

  /// Light theme.
  static ThemeData light() => ThemeData(
        useMaterial3: true,
        colorScheme: ColorScheme.fromSeed(seedColor: _seed),
      );

  /// Dark theme.
  static ThemeData dark() => ThemeData(
        useMaterial3: true,
        colorScheme: ColorScheme.fromSeed(
          seedColor: _seed,
          brightness: Brightness.dark,
        ),
      );
}
