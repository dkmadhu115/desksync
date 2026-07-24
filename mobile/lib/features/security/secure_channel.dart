// The private fields are populated from named constructor parameters; Dart
// forbids private named parameters, so initializing formals don't apply.
// ignore_for_file: prefer_initializing_formals

import 'dart:convert';
import 'dart:typed_data';

import 'package:cryptography/cryptography.dart';

/// Which end of the channel this instance represents.
enum SecureRole {
  /// The controlling peer (this mobile client).
  controller,

  /// The controlled peer (the desktop agent).
  agent,
}

/// Failure reason for a [SecureChannel] operation.
enum SecureChannelError {
  /// Frame too short to contain a counter + tag.
  malformed,

  /// Counter not strictly greater than the last accepted one.
  replay,

  /// AEAD authentication/decryption failed.
  authentication,
}

/// Thrown by [SecureChannel.open] on invalid frames.
class SecureChannelException implements Exception {
  /// Creates the exception.
  const SecureChannelException(this.error);

  /// The reason.
  final SecureChannelError error;

  @override
  String toString() => 'SecureChannelException(${error.name})';
}

/// End-to-end encrypted, authenticated, replay-protected channel to the paired
/// desktop.
///
/// This is the Dart mirror of the agent's `desksync-crypto` crate and MUST stay
/// byte-for-byte compatible: X25519 ECDH → HKDF-SHA256 (fixed salt, info binds
/// session id + both public keys) → AES-256-GCM, with frames framed as
/// `counter(8, big-endian) || ciphertext||tag` and a 96-bit `0…0 || counter`
/// nonce. See the interop vector test.
class SecureChannel {
  SecureChannel._({
    required List<int> sendKey,
    required List<int> recvKey,
    required int sendDir,
    required int recvDir,
    required List<int> sessionId,
  })  : _sendKey = SecretKey(sendKey),
        _recvKey = SecretKey(recvKey),
        _sendDir = sendDir,
        _recvDir = recvDir,
        _sessionId = sessionId;

  static const int _keyLen = 32;
  static const int _nonceLen = 12;
  static const int _tagLen = 16;
  static const int _counterLen = 8;
  static const int _dirC2A = 1;
  static const int _dirA2C = 2;

  static final List<int> _hkdfSalt = utf8.encode('desksync-e2e-v1');
  static final AesGcm _aesGcm = AesGcm.with256bits(nonceLength: _nonceLen);

  final SecretKey _sendKey;
  final SecretKey _recvKey;
  final int _sendDir;
  final int _recvDir;
  final List<int> _sessionId;
  int _sendCounter = 0;
  int? _recvLast;

  /// Build a channel from an already-agreed shared secret.
  static Future<SecureChannel> fromSharedSecret({
    required List<int> sharedSecret,
    required String sessionId,
    required List<int> controllerPub,
    required List<int> agentPub,
    required SecureRole role,
  }) async {
    final sid = utf8.encode(sessionId);
    final c2a = await _derive('c2a', sharedSecret, sid, controllerPub, agentPub);
    final a2c = await _derive('a2c', sharedSecret, sid, controllerPub, agentPub);
    return role == SecureRole.controller
        ? SecureChannel._(sendKey: c2a, recvKey: a2c, sendDir: _dirC2A, recvDir: _dirA2C, sessionId: sid)
        : SecureChannel._(sendKey: a2c, recvKey: c2a, sendDir: _dirA2C, recvDir: _dirC2A, sessionId: sid);
  }

  /// Build a channel by performing X25519 ECDH with the peer's public key.
  static Future<SecureChannel> establish({
    required List<int> localSecret,
    required List<int> peerPublic,
    required String sessionId,
    required List<int> controllerPub,
    required List<int> agentPub,
    required SecureRole role,
  }) async {
    final shared = await x25519SharedSecret(localSecret, peerPublic);
    return fromSharedSecret(
      sharedSecret: shared,
      sessionId: sessionId,
      controllerPub: controllerPub,
      agentPub: agentPub,
      role: role,
    );
  }

  /// Encrypt [plaintext] into a self-framed sealed message.
  Future<Uint8List> seal(List<int> plaintext) async {
    final counter = _sendCounter;
    final box = await _aesGcm.encrypt(
      plaintext,
      secretKey: _sendKey,
      nonce: _nonceFor(counter),
      aad: _aad(_sendDir),
    );
    _sendCounter += 1;

    final frame = BytesBuilder(copy: false)
      ..add(_counterBytes(counter))
      ..add(box.cipherText)
      ..add(box.mac.bytes);
    return frame.toBytes();
  }

  /// Authenticate and decrypt a sealed message. Rejects replays/out-of-order
  /// frames; receive state only advances on a valid tag.
  Future<Uint8List> open(List<int> frame) async {
    if (frame.length < _counterLen + _tagLen) {
      throw const SecureChannelException(SecureChannelError.malformed);
    }
    final bytes = Uint8List.fromList(frame);
    final counter = _readCounter(bytes);
    final last = _recvLast;
    if (last != null && counter <= last) {
      throw const SecureChannelException(SecureChannelError.replay);
    }

    final cipherText = bytes.sublist(_counterLen, bytes.length - _tagLen);
    final mac = Mac(bytes.sublist(bytes.length - _tagLen));
    try {
      final clear = await _aesGcm.decrypt(
        SecretBox(cipherText, nonce: _nonceFor(counter), mac: mac),
        secretKey: _recvKey,
        aad: _aad(_recvDir),
      );
      _recvLast = counter;
      return Uint8List.fromList(clear);
    } on SecretBoxAuthenticationError {
      throw const SecureChannelException(SecureChannelError.authentication);
    }
  }

  static Future<List<int>> _derive(
    String prefix,
    List<int> shared,
    List<int> sessionId,
    List<int> controllerPub,
    List<int> agentPub,
  ) async {
    final info = <int>[...utf8.encode(prefix), ...sessionId, ...controllerPub, ...agentPub];
    final hkdf = Hkdf(hmac: Hmac.sha256(), outputLength: _keyLen);
    final key = await hkdf.deriveKey(secretKey: SecretKey(shared), nonce: _hkdfSalt, info: info);
    return key.extractBytes();
  }

  List<int> _aad(int dir) => <int>[..._sessionId, dir];

  static List<int> _nonceFor(int counter) {
    final nonce = Uint8List(_nonceLen);
    nonce.setRange(_nonceLen - _counterLen, _nonceLen, _counterBytes(counter));
    return nonce;
  }

  static List<int> _counterBytes(int counter) {
    final out = Uint8List(_counterLen);
    ByteData.view(out.buffer).setUint64(0, counter);
    return out;
  }

  static int _readCounter(Uint8List frame) => ByteData.view(frame.buffer, frame.offsetInBytes, _counterLen).getUint64(0);
}

/// Perform X25519 ECDH, returning the raw 32-byte shared secret. Provided for
/// the [SecureChannel.establish] path.
Future<List<int>> x25519SharedSecret(List<int> localSecret, List<int> peerPublic) async {
  final algo = X25519();
  final keyPair = await algo.newKeyPairFromSeed(localSecret);
  final shared = await algo.sharedSecretKey(
    keyPair: keyPair,
    remotePublicKey: SimplePublicKey(peerPublic, type: KeyPairType.x25519),
  );
  return shared.extractBytes();
}
