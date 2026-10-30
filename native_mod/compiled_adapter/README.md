# Native technology tools

This is a development kit for the optional tools bundled with Tech Tree &
Research Planner. The player does not need Python, Rust or a compiler.

- INSTALL-TECH-ADAPTER generates the in-game planner from the selected source
  Mods and updates this planner's owned archive after verified backup. Close
  Stellaris and its launcher first. It does not install or modify source Mods.
- OPEN-TECH-TREE opens a read-only browser reference. The currently published
  Workshop browser entry is Windows-only. It does not synchronize save progress
  or in-game goals. PDF export retains bilingual names and IDs as real text.

See launchers/ADAPTER-README.txt and launchers/WEB-ATLAS-README.txt for usage.
The supplied root launchers use detected defaults; confirm sources with Enter.
Rerun adaptation after changing source Mods or receiving a planner update.
Standard technology definitions are supported; unusual scripts and special
eligibility conditions remain game-controlled. These are build-time tools,
not live in-game discovery. They use no injection and upload no game data.

Own source is MIT licensed; third-party notices retain their original terms.
Native wrapper tests are separate from in-game and browser UI acceptance.
