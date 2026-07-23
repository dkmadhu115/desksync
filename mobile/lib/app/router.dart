import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../features/auth/presentation/login_screen.dart';
import '../features/devices/presentation/device_list_screen.dart';
import '../features/pairing/presentation/pairing_screen.dart';
import '../features/viewer/presentation/desktop_viewer_screen.dart';

/// Named route paths, centralized to avoid stringly-typed navigation bugs.
abstract final class Routes {
  static const login = '/login';
  static const devices = '/devices';
  static const pairing = '/pairing';

  /// Viewer route template; use [viewerPath] to build a concrete path.
  static const viewer = '/viewer/:deviceId';

  static String viewerPath(String deviceId) => '/viewer/$deviceId';
}

/// Provides the app's [GoRouter]. Kept in a provider so redirect logic can
/// later depend on auth state (Phase 4) without rebuilding the widget tree.
final routerProvider = Provider<GoRouter>((ref) {
  return GoRouter(
    initialLocation: Routes.login,
    routes: [
      GoRoute(
        path: Routes.login,
        builder: (context, state) => const LoginScreen(),
      ),
      GoRoute(
        path: Routes.devices,
        builder: (context, state) => const DeviceListScreen(),
      ),
      GoRoute(
        path: Routes.pairing,
        builder: (context, state) => const PairingScreen(),
      ),
      GoRoute(
        path: Routes.viewer,
        builder: (context, state) => DesktopViewerScreen(
          deviceId: state.pathParameters['deviceId'] ?? 'unknown',
        ),
      ),
    ],
  );
});
