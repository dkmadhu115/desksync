import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/network/api_exception.dart';
import '../data/pairing_repository.dart';
import '../domain/pairing.dart';

/// Phase of the pairing confirmation flow.
enum PairingPhase {
  /// No action taken yet.
  idle,

  /// A confirmation request is in flight.
  submitting,

  /// Pairing confirmed successfully.
  success,

  /// The last attempt failed.
  error,
}

/// Immutable UI state for the pairing screen.
class PairingUiState {
  /// Creates a pairing UI state.
  const PairingUiState({
    this.phase = PairingPhase.idle,
    this.message,
    this.pairing,
  });

  /// The current phase.
  final PairingPhase phase;

  /// An error/info message to display.
  final String? message;

  /// The confirmed pairing, when [phase] is [PairingPhase.success].
  final Pairing? pairing;

  /// Whether a request is in flight.
  bool get isSubmitting => phase == PairingPhase.submitting;
}

/// Drives the pairing confirmation flow. QR scanning and the full trust
/// handshake arrive in Phase 6; here we implement manual-code confirmation,
/// which follows the same backend contract.
class PairingController extends Notifier<PairingUiState> {
  PairingRepository get _repo => ref.read(pairingRepositoryProvider);

  @override
  PairingUiState build() => const PairingUiState();

  /// Confirm a pairing using the pairing id and 8-digit code shown by the
  /// desktop agent.
  Future<void> confirm({
    required String pairingId,
    required String code,
  }) async {
    final id = pairingId.trim();
    final normalizedCode = code.trim();
    if (id.isEmpty || normalizedCode.isEmpty) {
      state = const PairingUiState(
        phase: PairingPhase.error,
        message: 'Enter both the pairing id and the code from your laptop.',
      );
      return;
    }

    state = const PairingUiState(phase: PairingPhase.submitting);
    try {
      final pairing = await _repo.confirm(pairingId: id, code: normalizedCode);
      state = PairingUiState(
        phase: PairingPhase.success,
        message: 'Device paired successfully.',
        pairing: pairing,
      );
    } on ApiException catch (e) {
      state = PairingUiState(phase: PairingPhase.error, message: e.message);
    } catch (_) {
      state = const PairingUiState(
        phase: PairingPhase.error,
        message: 'Unexpected error. Please try again.',
      );
    }
  }

  /// Reset back to the idle state.
  void reset() => state = const PairingUiState();
}

/// Provides the [PairingController].
final pairingControllerProvider =
    NotifierProvider<PairingController, PairingUiState>(PairingController.new);
