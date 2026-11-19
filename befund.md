# Befund

Das Vorhaben ist machbar. Der Detached Host ist bereits ein einzelner Prozess pro Datenverzeichnis, und mehrere App-Prozesse können sich mit ihm verbinden. Die Einschränkung liegt beim Attach einer einzelnen Session: Ein neuer Client verdrängt den bisherigen. Die aktuelle Implementierung und ein Integrationstest halten dieses Verhalten ausdrücklich fest. ADR 0009 (docs/adr/0009-detached-session-host.md:78), server.rs (crates/terminal-host/src/server.rs:545), host.rs (crates/terminal-host/tests/host.rs:350)

Tabs und Spaces sind aktuell noch kein geteilter Live-Zustand. Sie leben in jedem App-Prozess lokal. Beim Start wird der gesamte Space-Snapshot aus einer einzelnen session.json geladen; Spaces haben keine dauerhaften IDs, und der Snapshot speichert den aktiven Space als Index. Gleichzeitiges Speichern kann daher Änderungen überschreiben. spaces.rs (crates/workspace/src/spaces.rs:26), session_v3.rs (crates/workspace/src/session_v3.rs:246), session.rs (crates/workspace/src/session.rs:537)

Das ist eine Erweiterung von ADR 0009: Multi-Client-Mirroring steht dort ausdrücklich außerhalb des damaligen Umfangs. Die Workspace-Seite muss ebenfalls angepasst werden; allein den Host umzubauen würde noch keine live geteilten Tabs oder Spaces ergeben.

## Was schon vorhanden ist

- Ein Daemon pro Nutzer-Datenverzeichnis, abgesichert durch einen Prozess-Lock; jeder App-Prozess bekommt seine eigene IPC-Verbindung. Das ist eine gute Grundlage für installierte und Entwicklungs-Versionen desselben Nutzers. spawn.rs (crates/terminal-host/src/spawn.rs:109), client.rs (crates/terminal-host/src/client.rs:175)
- Der Daemon hält PTY und Schatten-Emulator. Jeder Client hat bereits eine begrenzte Ausgabewarteschlange, die nicht den PTY-Leser blockieren soll.
- Die App führt ihren eigenen Terminal-Emulator. Auswahl, Suche und Scrollposition sind damit heute schon lokal pro Ansicht; PTY-Ausgabe und Eingabe laufen über den Host.
- Der aktuelle Snapshot speichert alle Spaces einer App. Schreiben ist jedoch an terminal.sessionHost = detached und general.sessionRestore = true gebunden; sessionRestore ist standardmäßig ausgeschaltet. workspace.rs (crates/workspace/src/workspace.rs:1328), general.rs (crates/settings-content/src/general.rs:59)
- Der Scrollback-Cleanup berücksichtigt nur die Pane-IDs des aktuellen Workspace-Prozesses. Mit mehreren Instanzen könnte eine Instanz gespeicherte Daten einer anderen als verwaist löschen. scrollback.rs (crates/terminal-core/src/scrollback.rs:130)

## Empfohlene Aufteilung

This Mermaid diagram doesn't fit the current terminal width.
flowchart LR
    A["App A: Workspace und Terminalansicht"] -->|"Space beanspruchen und Änderungen"| H["Gemeinsamer Host"]
    B["App B: Workspace und Terminalansicht"] -->|"Space beanspruchen und Änderungen"| H
    A -->|"Eingabe und Resize"| H
    B -->|"Eingabe und Resize"| H
    H -->|"PTY-Ausgabe an alle Ansichten"| A
    H -->|"PTY-Ausgabe an alle Ansichten"| B
    H --> P["Eine PTY-/SSH-Session"]

- Terminal verantwortet die Mehrfachansichten einer Session: Ausgabe verteilen, Eingaben entgegennehmen und Session-Lebensdauer regeln.
- Workspace bleibt Eigentümer von Space-, Tab-, Pane- und Layout-Zustand. Dieser Zustand soll nicht in einen „Host-Gottservice“ im Shell-Modul wandern.
- Shell verbindet die Verträge. Die genaue IPC-Grenze für Workspace-Abstimmung braucht vor der Umsetzung eine ADR-Entscheidung, damit Workspace und Terminal keine privaten Zustände des jeweils anderen übernehmen.

## Zielverhalten, das ich empfehlen würde

1. Beim Start beansprucht jede App-Instanz atomar einen Space. Die erste nimmt den zuletzt verwendeten freien Space. Weitere Instanzen nehmen einen anderen freien Space; wenn keiner verfügbar ist, wird ein leerer Space angelegt.
2. Der neue Space übernimmt nach Möglichkeit die Projektidentität des zuletzt verwendeten Spaces, aber keine kopierten Tabs oder Terminal-Sessions. So bleibt der Kontext vertraut, ohne versehentlich Duplikate zu starten.
3. Eine App kann über die bestehende Space-Auswahl ausdrücklich einem bereits verwendeten Space beitreten. Dann sehen beide Instanzen dieselbe Tab- und Pane-Struktur und können dieselbe Terminal-Session anzeigen.
4. Tab-/Pane-Struktur, Reihenfolge und Terminal-Session-Referenzen sind geteilt. Fokus, Textauswahl, Scrollposition und Fensterzustand bleiben zunächst pro Instanz lokal. Beide können also dasselbe Terminal öffnen, ohne dass ein Fokuswechsel in der anderen App die Ansicht ständig umschaltet.
5. Eine Terminal-Session hat einen PTY und nimmt Eingaben von allen angehängten Ansichten an. Eingabeframes werden vom Host geordnet weitergegeben; gleichzeitig getippte Tasten können sich aber vermischen. Dafür muss festgelegt werden, ob paralleles Tippen frei erlaubt ist oder ob es einen sichtbaren Schreib-/Steuerungs-Lease gibt.
6. Tab schließen und Session beenden werden getrennt behandelt: Eine Ansicht zu schließen soll nicht ohne Weiteres eine Session beenden, die eine andere Instanz noch verwendet. Das explizite Beenden einer Session wirkt global.

## Umsetzungsplan

1. Produktvertrag und ADR festlegen

Vor Code die offenen Semantiken entscheiden: Space-Zuweisung, Verhalten beim Beitritt zu einem belegten Space, gemeinsamer oder lokaler aktiver Tab, paralleles Tippen, Schließen versus globales Beenden, Umgang mit sessionRestore und globalem „Quit and stop all sessions“. Danach ADR 0009 ergänzen oder ablösen und den Workspace-Vertrag aktualisieren.

2. Mehrere Ansichten pro Terminal-Session ermöglichen

Den einzelnen Sink einer Session durch mehrere abonnierte Clients ersetzen. Der Host verteilt Ausgabe und Snapshots an jeden Client; ein langsamer Client erhält unabhängig von den anderen einen Resync. Eingabe ist von jedem gültigen Teilnehmer erlaubt. Die heutige attached-Angabe müsste mindestens die Zahl der Ansichten abbilden oder ergänzen.

Für die Terminalgröße braucht es eine klare Regel: Eine PTY hat nur eine Dimension. Wenn zwei Fenster unterschiedliche Größen haben, dürfen beide nicht unkoordiniert ständig Resize auslösen. Ich würde einen expliziten Steuerungs-Lease vorsehen: Nur die aktive Ansicht bestimmt Resize; alle anderen können lesen und tippen.

3. Spaces und Workspace-Zustand mehrprozessfähig machen

- Dauerhafte, prozessübergreifend eindeutige IDs für Spaces sowie für synchronisierte Tabs und Panes einführen.
- Aktive Space-Auswahl pro App-Instanz speichern, nicht als globalen Index im gemeinsamen Workspace-Snapshot.
- Space-Belegung atomar verwalten. Eine Belegung muss bei sauberem Schließen und Verbindungsabbruch freigegeben werden; eine abgestürzte App darf einen Space nicht dauerhaft blockieren.
- Gemeinsame Zustandsänderungen versionieren. Ändern zwei Instanzen denselben Space, darf nicht einfach der letzte vollständige Snapshot gewinnen. Typed Commands oder Revisionen mit Konflikterkennung sollten veraltete Änderungen erkennen und den aktuellen Zustand neu laden.
- Die bestehende Snapshot-Migration auf dauerhafte IDs und parallele Speicherung ausrichten. Scrollback-Cleanup muss Referenzen aus allen Spaces und aktiven Host-Sessions berücksichtigen.
- Live-Updates brauchen eine erneute Zustandsaufnahme nach IPC-Abbruch, damit ein Client verpasste Events nicht stillschweigend übersieht.

4. Oberfläche und Lebenszyklus ergänzen

Die Space-Auswahl sollte anzeigen, welche Spaces gerade genutzt werden und das bewusste Beitreten ermöglichen. Das passt in eine vorhandene Workspace-Fläche und benötigt keine neue permanente Shell-Zone. Beim Schließen eines geteilten Tabs muss die Änderung an andere Teilnehmer weitergegeben werden; ob die Session danach weiterläuft, hängt von verbleibenden Referenzen oder einem expliziten Beenden ab.

Host-Key-, Authentifizierungs- und Transferkonflikt-Prompts brauchen außerdem genau einen zuständigen Antwortpfad. Werden Ereignisse an mehrere Instanzen gesendet, dürfen nicht mehrere Dialoge unabhängig dieselbe Entscheidung abschicken.

5. Kompatibilität für Stable und Dev absichern

Beide Builds teilen aktuell dasselbe nutzerbezogene Datenverzeichnis, aber eine inkompatible Host-Version kann den Anschluss verhindern. Die Protokollverhandlung muss ein unterstütztes Kompatibilitätsfenster garantieren; ein laufender Host mit Sessions sollte nicht automatisch für einen anderen Build ersetzt werden. Tests sollten Stable→Dev und Dev→Stable sowie parallele Starts abdecken.

## Wichtige Edge Cases

| Fall | Erforderliches Verhalten |
|---|---|
| Zwei Apps starten gleichzeitig | Atomare Space-Beanspruchung; kein doppelter Start desselben Spaces |
| App stürzt ab | Verbindung löst die Belegung; andere Instanz bekommt beim Reconnect einen vollständigen Snapshot |
| Eine App schließt ein Terminal-Tab | Kein unbeabsichtigtes Töten einer Session, die eine andere Ansicht verwendet |
| Zwei Fenster haben andere Größen | Eindeutige Resize-Regel für die eine PTY |
| Beide tippen gleichzeitig | Geordnete Frames; erwartbares Verhalten bei ineinanderlaufenden Eingaben |
| Space wird gelöscht, während andere App darin ist | Löschen ablehnen oder explizites Verlassen/Übernehmen verlangen |
| Host-Key-/Transferdialog erscheint in beiden Apps | Zuständiger Client oder at-most-once-Antwort |
| Langsamer Client verliert Ausgabe | Nur diesen Client resynchronisieren |
| App-/Host-Versionen unterscheiden sich | Kein erzwungener Host-Neustart mit laufenden Sessions |
| Scrollback einer anderen Space wird bereinigt | Nur anhand des globalen Workspace-/Host-Zustands löschen |
| sessionRestore ist ausgeschaltet | Live-Sharing während der Laufzeit trotzdem ermöglichen; Persistenz nach dem letzten App-Ende separat regeln |

## Umfangsgrenzen und Reihenfolge

Echte gleichzeitige Bearbeitung desselben Editor-Dokuments wäre ein eigener Schritt: Die Editor-Buffer und ungespeicherten Änderungen sind nicht bereits als gemeinsames Live-Modell angelegt. Gemeinsame Terminal-Ansichten und gemeinsame Tab-/Pane-Struktur lösen das nicht automatisch.

Die Zukunft mit einer Handy-App ist ein guter Grund, stabile IDs und einen transportneutralen Workspace-Vertrag vorzusehen. Sie rechtfertigt aber noch keinen offenen Netzwerk-Host. Der aktuelle Host vertraut auf lokalen Unix-Socket und Nutzer-ID; mobiler Zugriff erfordert später Pairing, authentifizierten Transport, Rechte pro Gerät und Widerruf.

Die aktive Rework-Warteschlange steht noch bei R07-000; R07-001 folgt danach. Eine Implementierung darf diese Reihenfolge nicht überspringen. tasks/rework/README.md (tasks/rework/README.md:28)

Ich habe nur recherchiert und geplant; es wurden keine Dateien geändert und keine Tests ausgeführt
