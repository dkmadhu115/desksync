import 'dart:convert';
import 'dart:math';

/// Generates a cryptographically-random key of [byteLength] bytes, base64
/// (standard) encoded. Used for the device identity key uploaded at
/// registration. Uses [Random.secure] so the value is unpredictable.
String generateRandomKeyBase64([int byteLength = 32]) {
  final rng = Random.secure();
  final bytes = List<int>.generate(byteLength, (_) => rng.nextInt(256));
  return base64Encode(bytes);
}
