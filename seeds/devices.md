# Device support catalog

Prefix `DEV`. These are the instruments and data sources the platform must support. HAL nodes must cover every row, or the row must be listed in out-of-scope.md. Integration paths:
- `IPP`: the I++ DME protocol;
- `SDK`: a vendor SDK through a driver host;
- `FILE`: the device's exported files, read through IO;
- `STD`: an open standard protocol.

Compiled 2026-11-16. **All rows are `[unverified]`** until someone confirms the protocol and SDK terms (founder-decisions, D gaps).

| ID | Device or class | Path · priority · phase |
|---|---|---|
| DEV.CMM.01 | CMM controllers with an I++ DME server (e.g. Renishaw UCC family) | IPP · high · P2 |
| DEV.CMM.02 | Other vendors' CMM controllers that expose an I++ server | IPP · high · P2 |
| DEV.CMM.03 | Manual CMMs and retrofits through an encoder interface | SDK · medium · P3 |
| DEV.CMM.04 | Rotary tables driven through the CMM controller | IPP · medium · P2 |
| DEV.CMM.05 | Probe heads, changers and scanning probes as reported by the controller | IPP · high · P2 |
| DEV.TRK.01 | Laser trackers, vendor A (e.g. Leica AT series) | SDK · high · P3 |
| DEV.TRK.02 | Laser trackers, vendor B (e.g. FARO Vantage) | SDK · high · P3 |
| DEV.TRK.03 | Laser trackers, vendor C (e.g. API Radian) | SDK · medium · P3 |
| DEV.TRK.04 | Tracker 6DoF probes and tracker-mounted scanners | SDK · medium · P3 |
| DEV.ARM.01 | Portable measuring arms, vendor A (e.g. FARO) | SDK · high · P3 |
| DEV.ARM.02 | Portable measuring arms, vendor B (e.g. Hexagon Absolute Arm) | SDK · high · P3 |
| DEV.ARM.03 | Arm-mounted laser line scanners | SDK · high · P3 |
| DEV.LIN.01 | Industrial line profile sensors with an open SDK (e.g. LMI Gocator) | SDK · high · P2 |
| DEV.LIN.02 | Line profile sensors (e.g. Micro-Epsilon scanCONTROL) | SDK · medium · P2 |
| DEV.LIN.03 | Line profile sensors (e.g. Keyence LJ series) | SDK · medium · P3 |
| DEV.ARE.01 | Structured-light and handheld scanners, through exported meshes and point clouds (ZEISS/GOM, Creaform, Shining3D, Scantech) | FILE · high · P1b |
| DEV.ARE.02 | Structured-light scanners with an open capture SDK | SDK · low · P4 |
| DEV.PHO.01 | Photogrammetry systems, through exported coordinate files | FILE · medium · P3 |
| DEV.TST.01 | Total stations and theodolites | SDK · low · P3 |
| DEV.TST.02 | Indoor GPS-style large-volume positioning systems | SDK · low · P3 |
| DEV.GAU.01 | Digital gauges and calipers through a wireless or wired gauge interface (e.g. Mitutoyo U-WAVE, Digimatic) | SDK · high · P2 |
| DEV.GAU.02 | Bluetooth LE gauges and calipers | STD · medium · P2 |
| DEV.GAU.03 | Gauge data through keyboard-wedge or serial (RS-232) | STD · high · P2 |
| DEV.ENV.01 | Temperature sensors for part and machine compensation | STD · medium · P2 |
| DEV.CAM.01 | Industrial cameras through GenICam for vision measurement | STD · low · P3 |
| DEV.CTL.01 | Shop-floor integration through OPC UA and MTConnect | STD · medium · P4 |
| DEV.SIM.01 | Simulated CMM (I++ simulator) | IPP · high · P0 |
| DEV.SIM.02 | Simulated tracker, arm and scanner | SDK · high · P2 |
