# Release notes

## 1.0.0

First release: the formant section of NyashkTune 1.x as its own plugin, reworked.

- Transpose / Detune, Formant Shift / Tilt / Follow, KEEP FORMANTS, five algorithms, Air / Drive, Mix, Output, voice range.
- New envelope estimation ("true envelope" from ~3 periods of the input): the formants move more accurately, most of all for low and high voices.
- The filtered grains are overlap-added whole, tails included: no clicks with sharp envelopes.
- AUTO GAIN keeps the level when transposing or shifting (was up to −6 dB for +12 st, −13 dB for a high voice).
- Neutral settings pass the input bit-exact.
- Above 8 kHz the formant filter boosts less and less (none above 16 kHz): big upward shifts with Tilt no longer turn the top octave into hiss.
- Live formant view: input and output envelopes with the formant peaks.
