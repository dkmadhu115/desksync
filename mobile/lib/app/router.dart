import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../features/auth/application/auth_controller.dart';
import '../features/auth/domain/auth_state.dart';
import '../features/auth/presentation/login_screen.dart';
import '../features/devices/presentation/device_list_screen.dart';
import '../features/devtools/presentation/quick_launch_screen.dart';
import '../features/pairing/presentation/pairing_screen.dart';
import '../features/viewer/presentation/desktop_viewer_screen.dart';
import 'splash_screen.dart';

/// Named route paths, centralized to avoid stringly-typed navigation bugs.
abstract final class Routes {
  static const splash = '/';
  static const login = '/login';
  static const devices = '/devices';
  static const pairing = '/pairing';
  static const quickLaunch = '/quick-launch';

  /// Viewer route template; use [viewerPath] to build a concrete path.
  static const viewer = '/viewer/:deviceId';

  static String viewerPath(String deviceId) => '/viewer/$deviceId';
}

/// Provides the app's [GoRouter] with an auth-aware redirect.
///
/// The redirect gates the whole app on [AuthStatus]: while the launch bootstrap
/// runs we show the splash; unauthenticated users are forced to the login
/// screen; authenticated users are kept out of login/splash. A [ValueNotifier]
/// bridges Riverpod auth-state changes to GoRouter's `refreshListenable` so the
/// redirect re-runs without rebuilding (and losing) the router.
final routerProvider = Provider<GoRouter>((ref) {
  final refresh = ValueNotifier<int>(0);
  ref.listen<AuthStatus>(
    authControllerProvider.select((s) => s.status),
    (_, _) => refresh.value++,
  );
  ref.onDispose(refresh.dispose);

  return GoRouter(
    initialLocation: Routes.splash,
    refreshListenable: refresh,
    redirect: (context, state) {
      final status = ref.read(authControllerProvider).status;
      final loc = state.matchedLocation;
      final atSplash = loc == Routes.splash;
      final atLogin = loc == Routes.login;

      switch (status) {
        case AuthStatus.unknown:
          return atSplash ? null : Routes.splash;
        case AuthStatus.unauthenticated:
        case AuthStatus.authenticating:
          return atLogin ? null : Routes.login;
        case AuthStatus.authenticated:
          return (atLogin || atSplash) ? Routes.devices : null;
      }
    },
    routes: [
      GoRoute(
        path: Routes.splash,
        builder: (context, state) => const SplashScreen(),
      ),
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
        path: Routes.quickLaunch,
        builder: (context, state) => const QuickLaunchScreen(),
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
