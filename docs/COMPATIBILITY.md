# Milan Pro compatibility

The plug-in and VEGAS-version matrix comes with batch R55 (`docs/REMAINING.md`). This file starts with the audio
devices (batch R9).

## Audio devices

Preferences › Audio Device › Audio device type:

| Type | What it is | VEGAS |
|---|---|---|
| Microsoft Sound Mapper (default) | The Windows default playback device, through WASAPI shared mode (cpal) | Same name and behaviour |
| Windows Classic Wave Driver | The playback device you choose, through WASAPI shared mode | Same name; VEGAS uses MME underneath |
| Windows Audio (Exclusive Mode) | The chosen device opened in WASAPI exclusive mode at its smallest period (or the Buffer size), for low-latency playback and input monitoring. Other programs go silent on that device while Milan Pro runs. | Not in VEGAS (Milan Pro's own, off by default) |
| ASIO | Not available | VEGAS offers it |

**ASIO is not supported.** cpal's `asio` feature builds against the Steinberg ASIO SDK, whose licence does not allow
it to be redistributed in an open-source project. Use Windows Audio (Exclusive Mode) for low latency instead.

Other limits:
- Playback is stereo. The rear and center/LFE device choices come with the multichannel engine (R10a).
- Recording and input monitoring use one stereo input (the Default audio recording device). Input monitoring needs
  the recording and playback devices at the same sample rate; otherwise the status bar says so and nothing is heard.
- The latency figures on the Audio Device page are what the devices report. Check them with a loopback cable
  (an output wired to an input) and a recording of a click if you need sample accuracy.
