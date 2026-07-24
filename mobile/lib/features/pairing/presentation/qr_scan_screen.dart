import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import '../domain/pairing_link.dart';

/// Full-screen camera QR scanner. Pops with the parsed [PairingLink] as soon as
/// a valid DeskSync pairing code is detected, or with null if the user cancels.
class QrScanScreen extends StatefulWidget {
  /// Creates the scanner screen.
  const QrScanScreen({super.key});

  @override
  State<QrScanScreen> createState() => _QrScanScreenState();
}

class _QrScanScreenState extends State<QrScanScreen> {
  final MobileScannerController _controller = MobileScannerController(
    detectionSpeed: DetectionSpeed.noDuplicates,
  );

  // Guards against emitting more than one result while the route unwinds.
  bool _handled = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _onDetect(BarcodeCapture capture) {
    if (_handled) return;
    for (final barcode in capture.barcodes) {
      final raw = barcode.rawValue;
      if (raw == null) continue;
      final link = PairingLink.tryParse(raw);
      if (link != null) {
        _handled = true;
        Navigator.of(context).pop(link);
        return;
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Scan pairing code')),
      body: Stack(
        fit: StackFit.expand,
        children: [
          MobileScanner(controller: _controller, onDetect: _onDetect),
          const _ScannerReticle(),
          const Positioned(
            left: 0,
            right: 0,
            bottom: 48,
            child: Text(
              'Point your camera at the QR code on your laptop',
              textAlign: TextAlign.center,
              style: TextStyle(color: Colors.white, fontSize: 16),
            ),
          ),
        ],
      ),
    );
  }
}

/// A simple centered square reticle to guide framing of the QR code.
class _ScannerReticle extends StatelessWidget {
  const _ScannerReticle();

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Container(
        width: 240,
        height: 240,
        decoration: BoxDecoration(
          border: Border.all(color: Colors.white70, width: 3),
          borderRadius: BorderRadius.circular(16),
        ),
      ),
    );
  }
}
