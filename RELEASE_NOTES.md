## 1.0.2

- Frequency grid labels read 1k / 2k / 10k instead of 1.0k / 2.0k / 10.0k (formant labels keep one decimal, e.g. 2.7k).

# Release notes

## 1.0.1

- A window size saved by an older version is opened with the right proportions.

## 1.0.0

First release: the formant section of NyashkTune 1.x as its own plugin, reworked.

- Transpose / Detune, Formant Shift / Tilt / Follow, KEEP FORMANTS, five algorithms, Air / Drive, Mix, Output, voice range.
- New envelope estimation ("true envelope" from ~3 periods of the input): the formants move more accurately, most of all for low and high voices.
- The filtered grains are overlap-added whole, tails included: no clicks with sharp envelopes.
- AUTO GAIN keeps the level when transposing or shifting (was up to −6 dB for +12 st, −13 dB for a high voice).
- Neutral settings pass the input bit-exact.
- Above 8 kHz the formant filter boosts less and less (none above 16 kHz): big upward shifts with Tilt no longer turn the top octave into hiss.
- Live formant view: input and output envelopes with the formant peaks.
