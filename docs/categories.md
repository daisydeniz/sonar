# Categories

The category taxonomy of `sonar_core::classify::category` (T5.3, ADR-028).
The Rust table `classify/category/taxonomy.rs` is the source of truth; this
page mirrors it (a unit test checks that every id, prompt and name word
below is here).

Category is the **sound source**; loop / one-shot is a separate field. A
kick loop is `kick`, a snare fill is `snare`; `drums` is for material with
several drums together (drum loops, top loops, fills, breaks).

Stored in `analysis.category` (id), `category_conf` (`0..1`) and
`category_source` (`detected` | `filename`). Effective value = user
override ?? stored category (ARCHITECTURE §3).

## Taxonomy

Name words are matched as the search tokenizer folds file names (case,
accents and plurals folded, `HiHat` → `hi hat hihat`); a multi-word entry
matches consecutive words. *Weak* words only count when no other category
word is in the same name segment.

| Id | Meaning | CLAP prompts (class score = best prompt) | Name words | Weak words |
|---|---|---|---|---|
| `kick` | kick / bass drum | a kick drum; an electronic kick drum | `kick` `kik` `bd` `bassdrum` `bass drum` | |
| `snare` | snare drum, rolls, snare fills | a snappy snare; a snare roll | `snare` `sd` `snr` | |
| `clap` | hand clap, finger snap | a hand clap; the sound of a clap; a finger snap; a clap; an electronic clap; people clapping; a layered handclap with reverb; a drum machine clap | `clap` `clp` `handclap` `snap` `fingersnap` | |
| `hat` | hi-hat, open or closed | a hi-hat; the sound of a closed hi-hat; an open hi-hat; a closed hi-hat; a hi-hat loop; hi-hats | `hat` `hihat` `hh` `ohh` `chh` | |
| `cymbal` | crash, ride, splash, china | a crash cymbal; a ride cymbal; the sound of a cymbal; a cymbal crash; a reverse cymbal; a ride cymbal bell | `cymbal` `cym` `crash` `ride` `splash` `china` | |
| `tom` | tom-tom | the sound of a tom-tom drum hit | `tom` `tomtom` | |
| `perc` | other percussion | a percussion hit; a shaker; a conga drum; a tambourine; a bongo; a cowbell; a rimshot; a woodblock; a percussion loop; a click; a short metallic percussion hit | `perc` `percussion` `percusion` `shaker` `conga` `bongo` `tambourine` `tamb` `cowbell` `clave` `woodblock` `timbale` `guiro` `cabasa` `rim` `rimshot` | |
| `drums` | several drums together | a drum loop; a drum beat; a drum fill; a breakbeat; drums playing a groove; a drum kit pattern | `drumloop` `breakbeat` | `drum` `top` `fill` `break` `kit` |
| `bass` | bass, sub, 808 bass | a bass synth; a bass line; an 808 bass; a sub bass; a deep bass note; a distorted bass; an electric bass guitar | `bass` `bassline` `subbass` `reese` | `808` |
| `lead` | lead synth / melody | a synth lead melody; a lead synthesizer; a lead melody | `lead` | |
| `pad` | pad | a synth pad; a warm sustained pad chord; an ambient pad; a string pad | `pad` | |
| `keys` | piano, e-piano, organ | a piano; an electric piano; an organ; piano chords; a piano note; a rhodes | `keys` `piano` `epiano` `rhodes` `organ` `wurli` `wurlitzer` `clav` `clavinet` `keyboard` | |
| `pluck` | pluck | a plucked synth; a plucked string | `pluck` `plucked` | |
| `synth` | other synth parts: chords, stabs, arps | a synthesizer; synth chords; a synth arpeggio; a synth stab; an acid synth; a synth chord stab; a synth hit; an electronic synth sound | `synth` `syn` `stab` | `chord` `arp` `arpeggio` |
| `vocal` | voice | a human voice; a vocal sample; singing; a vocal chop; a person speaking | `vocal` `vox` `voc` `voice` `acapella` `chant` `choir` `adlib` `spoken` | |
| `fx` | sound effects | a sound effect; a riser sweep; an impact sound effect; a whoosh; a white noise sweep; a downlifter; a sci-fi zap; a boom impact; a rising noise sweep; a transition effect | `fx` `sfx` `effect` `riser` `uplifter` `downlifter` `impact` `whoosh` `swoosh` `sweep` `buildup` `boom` `zap` `laser` `transition` | `reverse` `noise` `drop` |
| `texture` | atmospheres, drones, ambience, foley | an ambient texture; an atmospheric drone; background ambience; a field recording; an atmosphere | `texture` `atmosphere` `atmo` `atmos` `ambience` `drone` `soundscape` `foley` | |

Genre names never count as a category: `future bass`, `drum bass` (also
`Drum & Bass`, `drum n bass`), `bass house`, `bass music`, `uk bass`.

## Name hint rules

File stem first, then up to two parent folders (nearest first); the first
segment that names exactly one category gives the hint. A segment naming
two (`Synth & Plucks`) gives none and the next folder is read. The hint is
an input of the classifier, never the answer by itself: it adds a
calibrated-logit bonus of 3 (odds ≈ 20 : 1), so it decides unless the audio
clearly says otherwise (ADR-028). The eval hides names.

## Label mapping for the eval

- **Fixtures** (`truth.category`): `kick` → `kick`, `snare` → `snare`,
  `hat_closed` / `hat_open` → `hat`, `drums` → `drums`, `synth` by kind
  (one-shot pluck → `pluck`, sustained tone → `pad`, chord loop →
  `synth`). `sine`, `click`, `noise` (test signals) and corrupt files have
  no category label.
- **Silver**: the name hint above, read from the path relative to the
  silver root (the analyzer never sees it).
- **Golden**: the `category` column of `labels.csv`: an id of this table;
  plurals and the name words also parse (`Hi-Hats` → `hat`, `percussion`
  → `perc`); anything else is a warning and the cell is ignored.
