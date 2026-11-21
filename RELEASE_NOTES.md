## 1.0.6

- Більше жодних випадкових тріщань через інші копії плагіна: в аудіопотоці бралося глобальне блокування (лишилося від демо-режиму для скріншотів), спільне для всіх копій. Коли Bitwig обробляв кілька доріжок паралельно, один аудіопотік міг чекати на інший і не встигати. Тепер перевірка без блокувань.

## 1.0.5

- Кілька копій плагіна в одному проєкті тепер відкриваються миттєво: раніше друге вікно відкривалося до 1,5 с, а закриття будь-якого вікна блокувало DAW до 1 с (вікно чекало, поки закриються всі вікна наших плагінів у процесі, а не лише своє). Через це в Bitwig перше вікно могло на секунду зникати.

## 1.0.4

- Той самий захист від октавних помилок детектора, що й у Tune: високі ноти та початки нот зі слабкою основною частотою більше не читаються на октаву вище або нижче.

## 1.0.3

- **Big downward transpositions fixed:** Transpose -12 st came out at the original pitch and -9 / -11 st were mostly lost: the grains were stretched to fill the wider spacing and so carried several of the original periods, rebuilding the original pitch. Grains now stay about one period long; every mode lands within 0.1 st from -12 to +12 st.
- New behaviour tests: Transpose / Detune land on pitch in all five modes, an octave down really is an octave down.

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
