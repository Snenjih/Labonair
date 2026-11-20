# T22-001: vendored `gpui` — Entscheidungs-Task (P4, Gate)

## Status
📋 Geplant

## Phase
21 — Decision-Gate

## Abhängigkeiten
T21-004 (Modularitäts- & Personalisierungs-Abnahme), T21-005 (Architektur-Doku)


## Kontext
- `gpui = "0.2.2"` + `gpui-component = "0.5.1"` (an 0.2.2 gepinnt) —
  `Cargo.toml` / `[workspace.dependencies]`.
  - `WindowOptions` ohne always-on-top / Window-Level, ohne Max-Size, ohne
    Parent-Window-Handle → Settings-Fenster kann nicht an Main hängen /
    -minimieren; keine Max-Größe.
  - kein per-Window-Hide (Close zerstört).
  - Linux-Client-Side-Decorations / Multi-Window-Feinheiten unklar.
  - Keymap-Kontext-Prädikat-/Chord-APIs evtl. nicht voll exportiert
    (offener Punkt aus T19-008).

## Anweisungen zur Umsetzung
1. **Bedarf sammeln** (`docs/adr/0003-gpui-vendoring.md`, Abschnitt „Kontext"):
   - Konkrete, **geplante** Features/Tasks, die eine 0.2.2 fehlende API
     brauchen. Kandidaten: Settings-Fenster-Parent-Bindung, always-on-top-
     Overlays, weitere Tool-Fenster (Component-Gallery zählt nicht — geht
     auch so), Linux-CSD, Keymap-Prädikate (T19-008 — prüfen, ob dort ein
     Workaround reicht oder ein echter Blocker bleibt).
   - Pro Bedarf: „Workaround in 0.2.2 möglich? Wie teuer? Wie schlecht?"
2. **Optionen bewerten**:
   - **A — bei 0.2.2 bleiben**: Kosten = Workarounds/Verzicht (Liste).
     Nutzen = null Wartungslast, `gpui-component` bleibt kompatibel.
   - **C — Hybrid**: bei 0.2.2 bleiben, aber die 1–2 wirklich blockierenden
     APIs über einen kleinen plattformspezifischen Shim (eigenes
     `objc`/`x11`/`wayland`-Snippet) nachrüsten, ohne ganz `gpui` zu tauschen.
3. **`gpui-component`-Abhängigkeit prüfen**: wie tief hängt `labonair-ui-kit`
   nach Phase 19 noch an `gpui-component`? (Nach T20-001 sollten viele
   Primitives eigen sein.) Je geringer, desto realistischer Option B/C.
4. **Prototyp (zeitboxen, ~1 Tag)**: einen `gpui`-Git-Pin in einem
   Wegwerf-Branch ausprobieren — kompiliert der Workspace? Was bricht an
   `gpui-component`? Ergebnis + Fehlerliste ins ADR.
5. **Empfehlung** formulieren (im ADR): A / B / C, mit Bedingungen
   („B nur wenn Feature X priorisiert wird UND `gpui-component`-Ersatz in
   Phase 19 zu ≥80 % steht").
6. **Kein Merge von Vendoring in dieser Task** — wenn die Empfehlung B/C ist,
   ein separates, klar abgegrenztes Folgeticket `tasks/archive/phase-22-*/T23-001`
   anlegen (mit dem Prototyp-Branch als Ausgangspunkt). Wenn A: die
   Workarounds als kleine Tickets festhalten und den Punkt schließen.
7. **`docs/architecture.md`** (Abschnitt „Performance-Leitplanken" / neuer
   Abschnitt „GPUI-Basis"): die Entscheidung + Begründung eintragen.

## Akzeptanzkriterien
- [ ] `docs/adr/0003-gpui-vendoring.md` existiert: Kontext (konkrete
      Bedarfe + Workaround-Bewertung), Optionen A/B/C mit Kosten/Nutzen,
      Prototyp-Ergebnis, Empfehlung mit Bedingungen.
- [ ] Der Prototyp-Branch (gpui-Git-Pin) ist ausgeführt und sein Ergebnis
      (kompiliert? was bricht?) dokumentiert.
- [ ] Klare Entscheidung: A (Punkt geschlossen + Workaround-Tickets) **oder**
      B/C (Folgeticket `T23-001` angelegt, Prototyp verlinkt).
- [ ] `docs/architecture.md` hält die Entscheidung fest.
- [ ] `master` bleibt auf `gpui = "0.2.2"` (kein Vendoring in dieser Task
      gemerged).
- [ ] Gates grün auf `master`: `cargo fmt --check`,
      `cargo check --workspace --all-targets`,
      `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace`.

## Notizen
- Standard-Erwartung: **Option A oder C**, solange kein Feature Multi-Window-
  Lifecycle zwingend braucht. B ist ein dauerhafter Wartungsposten und sollte
  eine hohe Hürde haben.
- Diese Task ist bewusst am Ende der Roadmap — nach dem Rework weiß man
  besser, welche APIs wirklich fehlen.

## Warnungen
- ⚠️ `gpui-component` ist der Knackpunkt. Ohne belastbaren Plan für dessen
  Ersatz/Fork ist Option B nicht seriös empfehlbar.
- ⚠️ Prototyp im **Wegwerf-Branch**, niemals auf `master` — der Pin würde CI
  und alle Contributor-Builds beeinflussen.

## Weiterführende Tasks
- (bedingt) `tasks/archive/phase-22-gpui-vendor/T23-001-*` — nur wenn ADR-Empfehlung B/C.
