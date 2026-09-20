# Audit: Fork-Neuerungen in JMAP Collaboration

Datum: 2026-09-20. Gegenstand: Codequalität, funktionale Korrektheit, Integration, Wartbarkeit und betroffene Sicherheitsgrenzen. Dies ist eine Dokumentation der Analyse, kein Fix-Commit.

## Zusammenfassung

**Der untersuchte Entwicklungsbranch ist in diesem Zustand nicht zur Freigabe empfohlen.** Zwei voneinander unabhängige Integrationsfehler verhindern das Öffnen beziehungsweise Laden des neuen Adressbuchs. Die Rust-Implementierung übernimmt sinnvolle bestehende Grenzen und Transportmechanismen, hat aber Fehler bei Query-Pagination, Leserechten und Namensprojektion. Die neuen isolierten View-Tests erfassen die tatsächliche App-/Service-Verdrahtung nicht ausreichend.

**Security-Verdikt für die geänderten Grenzen: BLOCK.** Der neue Datenpfad für servergelieferte Adressbuchnamen erreicht einen Rich-Text-Renderer, der unerwartete Ressourcenanforderungen ausführen kann. Der Renderer-Effekt ist lokal belegt; die normale Navigation dorthin ist am geprüften Commit noch durch die funktionalen Blocker versperrt. Eine erfolgreiche Ausnutzung über einen realen Server wird ausdrücklich nicht behauptet.

Es wurden keine Produktivdateien repariert, keine echten Mailkonten verwendet und keine Änderungen an externen Mailservern vorgenommen. Dieser Bericht enthält weder ausführbare Sicherheits-Payloads noch Angriffsanleitungen. Nach der zusätzlichen Einschränkung des Auftraggebers wurden keine weiteren Sicherheits-, Netzwerk- oder Belastungsproben durchgeführt; die abschließenden Prüfungen waren lokale funktionale QML-Szenarien und Quellenanalyse.

## 1. Vergleichsbasis und vollständiger Änderungsumfang

| Referenz | Geprüfter Commit |
| --- | --- |
| Original: `huacnlee/omamail`, `main` | `f2774a9f76a0b433e6e4b562fe14903929011586` |
| Fork: `cstreil/omamail`, `main` | `f2774a9f76a0b433e6e4b562fe14903929011586` |
| Fork: `jmap-collaboration` | `530cba9ee159a44a57e692d09d4d571f9d49e2e1` |
| Fork: `carddav-address-book` | `377897f1f145eedcfe1a3a8fbf7f0c86134400f2` |

Die beiden `main`-Branches sind identisch. Die sechs fork-eigenen Commits liegen auf `jmap-collaboration`; dessen Netto-Diff umfasst 25 Dateien mit 2.509 hinzugefügten und 118 entfernten Zeilen. `carddav-address-book` enthält zwei Dokumentationscommits und ist ein Vorfahr dieses Branches, keine zweite Implementierung. Die übrigen gleichnamigen Fork-/Original-Branches stimmten beim Abgleich überein. Die Aussagen unten beziehen sich auf den fixierten Entwicklungscommit, nicht auf einen veröffentlichten Release und nicht auf die Qualität des gesamten Originalprojekts.

[Vergleich der geprüften Commits](https://github.com/cstreil/omamail/compare/f2774a9f76a0b433e6e4b562fe14903929011586...530cba9ee159a44a57e692d09d4d571f9d49e2e1).

### Abdeckung aller 25 geänderten Dateien

| Bereich | Geprüfte Dateien |
| --- | --- |
| Rust-Kontakte und RPC | `src/providers/jmap/contacts.rs`, `src/providers/jmap/mailbox.rs`, `src/providers/jmap/mod.rs`, `src/contacts/mod.rs`, `src/backend/contacts.rs`, `src/backend/methods.rs`, `src/backend/mod.rs` |
| UI und Zustandsintegration | `ui/App.qml`, `ui/Service.qml`, `ui/components/ContactsView.qml`, `ui/components/ComposeView.qml`, `ui/components/MailboxSidebar.qml`, `ui/components/HeaderActions.qml`, `ui/components/RefreshButton.qml`, `ui/keys/Keymap.js` |
| Tests | `src/providers/jmap/mailbox_tls_test.py`, `ui/tests/qml/tst_contacts_view.qml`, `ui/tests/qml/tst_compose_recipients.qml`, `ui/tests/qml/tst_service_backend_path.qml`, `app/tests/qml/tst_standalone_composition.qml`, `tests/test_source.sh` |
| Build, Vertrag und Dokumentation | `Makefile`, `backend-api.json`, `docs/JMAP-COLLABORATION.md`, `docs/KEYS.md` |

Unveränderte Abhängigkeiten wurden mit betrachtet, insbesondere `ui/account/Navigation.js`, `ui/backend/Backend.qml`, der tatsächliche Standalone-Button und die Textformat-Guard-Tests. Kalenderimplementierung, Kontaktmutation und persistente Kontaktsynchronisation sind laut Branchplan spätere Arbeiten und wurden nicht als fehlende Implementierung dieses read-only-Slices bewertet.

## 2. Bewertung und Nachweisstufen

- **P1:** Vor Freigabe der neuen Funktion beheben; erheblicher Funktionsausfall oder relevante neue Sicherheitsgrenze.
- **P2:** Konkreter Korrektheits-, Bedienungs- oder Robustheitsfehler, vor breiter Nutzung beheben.
- **Laufzeit:** Mit dem bestehenden Code und synthetischen Eingaben beobachtet.
- **Isolierte Laufzeit:** Hinter den bekannten Blockern mit einer ausdrücklich beschriebenen Testvorrichtung geprüft; keine Behauptung einer aktuell durchgängigen normalen Bedienroute.
- **Quellenbefund:** Kontroll-/Datenfluss im geprüften Commit nachvollzogen, ohne entsprechende End-to-End-Laufzeitbehauptung.

Die beiden Adressbuch-Blocker werden als P1 und nicht P0 eingestuft: Sie blockieren die neue Funktion, nicht nachweislich den gesamten Mailclient. Frühere mündliche Zwischenbewertungen mit P0 werden damit präzisiert.

## 3. Findings

### F01 — P1: Der neue Navigationsweg fällt auf die Mailliste zurück

**Ort:** [App.qml:618–621](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/App.qml#L618-L621), [Navigation.js:20–31, 54–56, 155–156](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/account/Navigation.js#L20-L56). **Nachweis: Laufzeit, Node.**

`showContacts()` übergibt `contacts` an `Nav.replaceRoot`. `Navigation.js` kennt diese Art weder in `KINDS` noch in `ROOTS`; `entry()` normalisiert sie auf `list`. Daher führen Sidebar, Alt+K und Ctrl+Shift+K nicht zur Kontaktansicht.

Eine ungefährliche Reproduktion mit dem vorhandenen Modul-Loader lautet:

```sh
node -e 'const n=require("./ui/tests/load.js").load("account/Navigation.js"); console.log(JSON.stringify(n.replaceRoot([{kind:"list"}],"contacts")))'
```

Beobachtet: `[{"kind":"list"}]`; erwartet: ein Kontakt-Root. Die Kontrollprobe mit `calendar` lieferte korrekt einen Kalender-Root.

**Empfehlung:** Den neuen Root konsistent im Navigationsmodell aufnehmen und den realen App-Einstieg testen, nicht nur die Kontaktansicht direkt instanziieren.

### F02 — P1: Die Directory-Freigabe bindet an eine nicht definierte Property

**Ort:** [Service.qml:110, 422–449](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/Service.qml#L422-L449). **Nachweis: Laufzeit, Qt 6.11.2.**

`contactsDirectoryAvailable` liest `backendCanListContacts`, das im Service nicht deklariert ist. Die tatsächlich vorhandene API-6-Freigabe heißt `backendCanSuggestRemoteContacts`. Quellenabgleich und QML-Laufzeit zeigen den Fehler unabhängig von F01.

```text
Service.qml:422: ReferenceError: backendCanListContacts is not defined
ready=true, apiVersion=6, directoryAvailable=false
```

`refreshContactSources` und `refreshContactList` verweigern dadurch das Laden. `openContactDetail` prüft diese Property dagegen nicht direkt; die frühere Aussage, sämtliche Directory-Methoden würden unmittelbar an diesem Gate abbrechen, war zu weit gefasst.

**Empfehlung:** Eine definierte, dauerhaft an API 6 gebundene Freigabe verwenden. Den realen Service mit aktivem Konto und API 6 testen; API 5 muss weiterhin korrekt verweigert werden.

### F03 — P1: Adressbuchnamen erreichen den Standalone-Rich-Text-Renderer

**Ort:** [ContactsView.qml:139–150](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/components/ContactsView.qml#L139-L150), [Button.qml:37–48](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/app/qml/imports/qs/Ui/Button.qml#L37-L48), [contacts.rs:372–393](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/src/providers/jmap/contacts.rs#L372-L393). **Nachweis: isolierte Renderer-Laufzeit, Produktions-Button.**

Ein Name aus `AddressBook/get` wird als `Button.text` weitergereicht. Dessen inneres `Text` verwendet implizit `Text.AutoText`. Die neue Verbindung servergelieferter Namen mit dem bestehenden Button ist der relevante Fork-Unterschied. `clean()` entfernt Steuerzeichen, aber keine Markup-Semantik. Für Adressbuchnamen gilt hier `MAX_TEXT = 4096`; die frühere Angabe eines 64-Zeichen-Limits war falsch und bezog sich auf Beschriftungen anderer Kontaktfelder.

Eine bereits vor der zusätzlichen Einschränkung durchgeführte lokale, isolierte Probe mit der tatsächlichen Implementierung unter `app/qml/imports` beobachtete eine unerwartete Ressourcenanforderung. Es wurden nur synthetische Daten und ein eigener lokaler Empfänger verwendet. Payload und ausführbarer Sicherheitsreproducer werden nicht veröffentlicht.

**Erreichbarkeit:** F01 und F02 versperren aktuell den normalen UI-Pfad. Die Probe belegt den Renderer-Effekt, nicht eine vollständige Ausnutzung über ein reales Konto. Kein Nachweis von Credential-Abfluss oder Codeausführung; Omarchy-eigene externe UI-Komponenten wurden nicht separat geprüft.

**Empfehlung:** Servertext am Renderer ausdrücklich als PlainText behandeln. Der Guard in `tests/test_qml_text_format.py` erfasst nur bestimmte `ui/`-Verzeichnisse und `Text`-/`Label`-Muster, nicht diesen Weiterleitungspfad in `app/`; sein grünes Ergebnis widerlegt den Befund nicht.

### F04 — P2: Serverbegrenzte Query-Seiten werden als vollständiges Adressbuch ausgegeben

**Ort:** [contacts.rs:185–266](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/src/providers/jmap/contacts.rs#L185-L266), insbesondere die lokale Total-Berechnung in [69–105](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/src/providers/jmap/contacts.rs#L69-L105). **Nachweis: Rust-Laufzeit mit synthetischem TLS-Fixture in einer separaten Auditkopie.**

`card_ids` fragt einmal mit Position 0 ab. Auch wenn der Server weniger IDs als angefordert liefert und eine größere Gesamtzahl nennt, wird keine Folgeseite geholt. `contact_list` berechnet anschließend `total` aus den tatsächlich geholten Karten und verdeckt so die Unvollständigkeit. Für Vorschläge verwendet `cards_for` ebenfalls nur eine Query-Seite.

[RFC 8620 §5.5](https://www.rfc-editor.org/rfc/rfc8620.html#section-5.5) erlaubt dem Server ausdrücklich, das angeforderte Limit zu reduzieren. Die lokale Probe bot zwei Kontakte an, lieferte aber nur eine ID pro Seite. Beobachtet: eine Listenzeile, `total: 1`, und ebenfalls nur ein Vorschlag. Die zweite Seite blieb ungenutzt.

**Empfehlung:** Innerhalb der bestehenden Objekt- und Zeitgrenzen bis zum tatsächlichen Ende paginieren; Query-State, Position und Fortschritt validieren. Erst danach vollständig sortieren und die UI-Seite bilden, oder unvollständige Ergebnisse ausdrücklich kennzeichnen.

### F05 — P2: Nicht lesbare Quellen werden angeboten und können automatisch ausgewählt werden

**Ort:** [contacts.rs:337–385](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/src/providers/jmap/contacts.rs#L337-L385), [Service.qml:470–479](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/Service.qml#L470-L479). **Nachweis: Rust-Projektionsprobe plus UI-Quellenbefund.**

`sources_of` liefert auch Bücher mit `mayRead: false`. Die spätere Listenoperation verweigert genau diese Quellen. Die UI verwechselt ihre Variable `readable` mit Schreibbarkeit: Sie filtert nach `readOnly !== true`, nicht nach Leserechten.

Die Probe mit einem nicht lesbaren Buch zuerst und einem lesbaren, schreibgeschützten Buch danach lieferte beide Quellen mit `readOnly: true`. Der UI-Fallback wählt dann die erste, nicht lesbare Quelle. Unabhängig von der automatischen Auswahl erscheinen außerdem Buttons für Quellen, die grundsätzlich nicht geladen werden können.

**Empfehlung:** Leserechte vor der Quellenprojektion durchsetzen oder explizit übertragen und konsistent auswerten. Schreibschutz und Lesbarkeit getrennt behandeln.

### F06 — P2: Empfängervorschläge verlieren strukturierte Namen

**Ort:** [contacts.rs:395–417, 554–581](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/src/providers/jmap/contacts.rs#L554-L581). **Nachweis: Rust-Laufzeit.**

Die Listen-/Detailprojektion verwendet `display_name`, die Vorschlagsprojektion liest dagegen ausschließlich `name.full`. Für dieselbe Karte mit strukturierten Namensbestandteilen wurde Folgendes beobachtet:

```text
Liste:       {"name":"Jane Doe", "emails":["jd@example.test"]}
Vorschläge:  [{"name":"", "email":"jd@example.test"}]
```

Damit fehlen Anzeigename und Namenssuchbarkeit in den Empfängervorschlägen, obwohl der Name in der Kontaktansicht verfügbar ist.

**Empfehlung:** Die Namensprojektion gemeinsam verwenden und Komponenten-only-Daten absichern. Der vorhandene Komponenten-Fallback ist außerdem sehr eng auf wenige Arten und jeweils den ersten Treffer begrenzt; seine Vollständigkeit sollte gegen das tatsächlich unterstützte JSContact-Schema geprüft werden. Die weitergehende Behauptung aus dem Zwischenbericht über alle zulässigen Namensarten ist kein separat runtime-verifizierter Befund dieses Berichts.

### F07 — P2: Strukturierte RPC-Fehler verlieren ihre spezifische Benutzererklärung

**Ort:** [Service.qml:425–433](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/Service.qml#L425-L433), [Backend.qml:190–210](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/backend/Backend.qml#L190-L210). **Nachweis: QML-Laufzeit.**

`Backend` liefert Fehlerobjekte mit `code` und `message`; `contactErrorText` vergleicht das gesamte Argument mit Strings. Für das Objekt mit `message: "contacts_too_many"` erschien nur `The address book could not be read.`. Die Kontrollprobe mit dem nackten String lieferte dagegen `Too many contacts to list. Narrow the search.`.

Die implementierten spezifischen Hinweise sind damit im normalen RPC-Fehlerpfad unerreichbar; gerade bei der Kontaktanzahlgrenze fehlt die notwendige Handlungsanweisung.

**Empfehlung:** Das bekannte Fehlerobjekt vor der Zuordnung normalisieren, ohne beliebige Servertexte ungefiltert anzuzeigen.

### F08 — P2: Schließen eines Details entwertet eine unabhängige Listenanfrage

**Ort:** [Service.qml:484–538](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/Service.qml#L484-L538). **Nachweis: isolierte QML-Service-Laufzeit.**

Quellen-, Listen- und Detailanfragen teilen einen Generationszähler. `closeContactDetail()` erhöht diesen Zähler unabhängig davon, ob gerade eine neue Liste angefordert wurde. Eine danach eintreffende Listenantwort wird verworfen; eine Ersatzanfrage wird nicht ausgelöst.

Die Probe initialisierte eine alte Zeile, öffnete einen Detailzustand, forderte eine Suche nach Bob an, schloss das Detail und stellte dann die passende Listenantwort zu. Beobachtet: `rowsAfterCloseAndListReply: "old"`. Die neue Zeile wurde nicht übernommen.

Für diese Probe wurde ausschließlich im QML-Testkontext die in F02 fehlende Property bereitgestellt. Die originalen Service-Funktionen und Backend-Callbacks blieben unverändert; Transport und Konten waren synthetisch.

**Empfehlung:** Die Kontozuordnung als gemeinsame Grenze erhalten, aber voneinander unabhängige Listen-/Detailoperationen nicht durch dieselbe Abbruchgeneration entwerten.

### F09 — P2: Aktualisieren entfernt den Filter aus der Anfrage, nicht aus dem Suchfeld

**Ort:** [ContactsView.qml:33–54](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/components/ContactsView.qml#L33-L54), [Service.qml:470–491](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/Service.qml#L470-L491). **Nachweis: isolierte QML-Service-Laufzeit und View-Datenfluss.**

Nach einer Suche lädt der Refresh-Pfad zuerst die Quellen und anschließend ausdrücklich mit leerem Query. Die Ansicht behält ihren Suchtext bei. Die beobachteten RPC-Parameter wechselten von `{"source":"book","limit":200,"query":"Bob"}` zu `{"source":"book","limit":200}`.

Damit zeigt die UI unfiltrierte Ergebnisse unter einem weiterhin sichtbaren Suchbegriff. Die Quellenaktualisierung wählt zudem die Quelle neu, statt die bestehende Auswahl nach Möglichkeit zu erhalten.

**Empfehlung:** Den aktiven Query und die Quellenauswahl beim Refresh konsistent erhalten oder einen beabsichtigten Reset auch sichtbar im UI ausführen.

### F10 — P2: Der App-Fokus erreicht den Kontakt-Tastaturhandler nicht

**Ort:** [App.qml:1572–1599](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/App.qml#L1572-L1599), [ContactsView.qml:392–426](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/components/ContactsView.qml#L392-L426), [Keymap.js:19–37](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/ui/keys/Keymap.js#L19-L37). **Nachweis: isolierte App-QML-Laufzeit mit positiver Gegenprobe.**

Der Kontaktkontext existiert, aber `applyContextFocus()` parkt dessen Fokus auf `keyboardHome`. Die Kontaktansicht erwartet Navigationstasten in ihrem eigenen `Keys.onPressed`; die globalen j/k-Bindings gelten nur für Mail. Der vorhandene View-Test ruft Bewegungsfunktionen direkt auf und prüft diese Fokusnaht nicht.

Die Probe verwendete die bestehende App-Navigationsfixture, ergänzte ein synthetisches Kontaktmodell und setzte ausschließlich im Harness einen Kontakt-Root, um F01 zu umgehen. Die echte App meldete `view: "contacts"`, `context: "contacts"` und eine sichtbare Kontaktansicht. Nach j und Pfeil-ab blieb `selected: ""`. Nachdem der Harness ausdrücklich den inneren Kontakt-Fokus setzte, wählte dieselbe j-Taste korrekt `selected: "one"`.

Die Fixture verwendet gemockte Shellmodule und erzeugt weitere bekannte Warnungen wegen nicht vollständig nachgebildeter Service-/Theme-Properties. Die positive Gegenprobe belegt die Fokusabhängigkeit, ersetzt aber keinen vollständigen Omarchy-Desktoptest.

**Empfehlung:** Kontakte in die vorhandene Kontext-/KeyRouter-Architektur integrieren und echte Tasteneingaben über die App-Komposition testen.

### F11 — P2: Kartensammlung hat kein kumulatives Bytebudget

**Ort:** [contacts.rs:270–310](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/src/providers/jmap/contacts.rs#L270-L310), Vergleich mit [mailbox.rs:624–643](https://github.com/cstreil/omamail/blob/530cba9ee159a44a57e692d09d4d571f9d49e2e1/src/providers/jmap/mailbox.rs#L624-L643). **Nachweis: Quellenanalyse und bereits durchgeführte begrenzte lokale Größenprobe.**

Die HTTP-Schicht begrenzt jede Antwort auf 32 MiB. `cards()` sammelt jedoch rohe Karten über mehrere Antworten, bevor die Projektion deren Texte begrenzt. Anders als `get_emails()` wird die Gesamtgröße nicht geprüft. Eine begrenzte synthetische Probe akzeptierte drei Karten mit zusammen 37.748.736 Bytes allein in den Namensfeldern; es wurde kein Speicherausfall provoziert.

**Wichtige Grenze:** Der Verbrauch ist nicht mathematisch unbeschränkt. Kartenanzahl, Einzelantwort und Laufzeit sind bereits begrenzt. Belegt ist das fehlende kleine kumulative Bytebudget, nicht ein gemessener Prozessabsturz oder ein bestimmter maximaler RAM-Verbrauch. Eine frühere Formulierung „kumulativ unbegrenzt“ war zu pauschal.

**Empfehlung:** Ein geeignetes kumulatives Bytebudget entsprechend dem bestehenden Mailpfad prüfen, bevor weitere rohe Karten gesammelt werden, oder die Projektion früher vornehmen.

## 4. Weitere Beobachtungen und bewusst nicht bestätigte Aussagen

### Weiter sichtbare Mailflächen unter der Kontaktansicht

`App.qml:1950–1951` und `2052–2053` schließen die Kontaktansicht nicht aus ihren Sichtbarkeitsbedingungen aus; die Kontaktansicht liegt darüber. Das ist ein belegter Integrations-/Wartbarkeitshinweis. **Nicht belegt ist ein tatsächliches Auslösen verdeckter Mailaktionen per Klick.** Dieser weitergehende Verdacht aus dem Zwischenbericht wird nicht als bestätigtes Finding veröffentlicht; ein entsprechender Aktionstest wurde nicht durchgeführt.

### Dokumentation und Capability-Verweigerung

`docs/JMAP-COLLABORATION.md:128–131` verspricht leere Ergebnisse bei fehlender Kontaktfähigkeit. Nicht-JMAP-Konten erhalten tatsächlich leere Quellen-/Listenresultate; für JMAP-Konten ohne Contacts-Capability propagiert `contacts_setup` dagegen `contacts_not_supported`. Das sollte dokumentarisch oder vertraglich vereinheitlicht werden, ist aber kein weiterer schwerwiegender Fehler.

### Vermeidbare lokale Arbeit

`src/backend/contacts.rs:42–101` nutzt `account_and_local` auch für Quellen, Listen und Details und verwirft dabei den lokal gesammelten Kontaktbestand. Das erzeugt unnötige lokale Dateiarbeit in Remote-only-Aufrufen. Eine messbare Performanceverschlechterung wurde nicht behauptet oder benchmarked.

### Positiv bewertete Entscheidungen

- Der neue Transport nutzt die bestehende native JMAP-Schicht statt eines zweiten Clients in QML; Mail- und Contacts-Primärkonten werden getrennt aufgelöst.
- RPC-Parameter werden auf erlaubte Felder, Typen und Größen geprüft. Fehlende, doppelte und unangeforderte Karten-IDs werden nicht still akzeptiert.
- Die neue Funktion ist bewusst read-only; nicht implementierte Mutationen werden nicht vorgetäuscht.
- Die API-Erweiterung ist korrekt als ein unveröffentlichter Schritt beschrieben: `apiVersion = 6`, `releasedApiVersion = 5`, also exakt `releasedApiVersion + 1`. Die alten lokalen Vorschlagsaufrufe mit `{}` bleiben vorgesehen.
- Compose prüft die Eigentümerschaft des Vorschlagsbestands und hat dafür einen ergänzten Test.
- `HeaderActions` und `RefreshButton` führen die Refresh-Aktion zusammen; kein zusätzlicher belegter Fehler in diesen kleinen Extraktionen.
- Die vorhandenen Rust-TLS-Fixtures nutzen synthetische Daten und prüfen reale Transportpfade. Die wesentliche Testlücke liegt in der Integration zwischen Navigation, Service, View und Fokus.

## 5. Verifikationsprotokoll und Grenzen

Umgebung: macOS arm64, Node, Rust/Cargo und Qt 6.11.2 über PySide6-Essentials im Offscreen-/Software-Modus. `qmltestrunner` und `qmllint` waren lokal nicht verfügbar. Der publizierte Quellbaum blieb unverändert; ergänzende Experimente lagen außerhalb des Repositories, Rust-Experimentänderungen ausschließlich in einer separaten Wegwerfkopie.

| Prüfung | Ergebnis und Aussagegrenze |
| --- | --- |
| `cargo test --locked --lib contacts -- --nocapture` am geprüften Originalstand | 17 bestanden; gezielte Kontakttests, nicht die komplette Rust-Suite |
| `make test-js` | Vorhandene JavaScript-Suite bestanden; kein Nachweis vollständiger QML-Integration |
| `python3 tests/test_backend_release.py` | 26 Tests bestanden; Release-/Vertragswerkzeuge, nicht 26 erfolgreiche Live-API-Interaktionen |
| `python3 tests/test_qml_text_format.py` | Bestanden, aber ohne Abdeckung von F03 |
| `python3 tests/test_qml_names.py` | Bestanden |
| `bash tests/test_source.sh` | Bestanden |
| `cargo build --locked --bin omamail` | Erfolgreich |
| `python3 tests/test_backend_api.py --binary target/debug/omamail` | Fehlgeschlagen bei `advertised API method: agent.context`; macOS-Build bietet Linux-only-Agent-Methoden nicht an. Kein grüner vollständiger API-Gate behauptet |
| Node-Navigationsprobe | F01 bestätigt |
| Reale Service-Bindung in QML | F02 bestätigt |
| Produktions-Standalone-Button, isolierte lokale Probe | F03 bestätigt; keine reale Serverlieferung oder normal erreichbare App-Route behauptet |
| Drei zusätzliche Rust-Beobachtungsproben in Wegwerfkopie | 3 bestanden: serverbegrenzte Query, Namen/Leserechte, kumulative Kartenbytes. Die Assertions bestätigen beobachtete Defekte, keine behobenen Regressionen |
| Reale Service-Funktionen und Backend-Callbacks im synthetischen QML-Kontext | F07, F08 und F09 bestätigt; F02 nur im Testkontext umgangen |
| Reale App-Komposition mit vorhandenen Shell-/Service-Fixtures und QTest-Tasten | F10 mit positiver Fokus-Gegenprobe bestätigt; F01 nur im Harness umgangen |

**Nicht ausgeführt:** vollständiges `make test`, vollständiges `make validate`, vollständige QML-Suite mit dem vorgesehenen Runner, native Linux-/Windows-Desktop-Gates, echte Benutzer-Mailkonten, reale Stalwart-End-to-End-Rundreisen, Kontaktmutationen oder externe Sicherheitsprüfungen. Die frühere Aussage „komplette lokale Test-Suite“ wird ausdrücklich durch dieses präzise Protokoll ersetzt.

Der Sicherheitsbefund ist ein Grund, die betroffene Funktion nicht freizugeben; er ist kein Volltest sämtlicher Sicherheitsgrenzen des Originalprojekts. Ungeprüfte angrenzende Funktionen werden dadurch weder freigegeben noch pauschal als unsicher bezeichnet.

## 6. Empfohlene Bearbeitungsreihenfolge — keine Fixes vorgenommen

1. F01 und F02 korrigieren und App-/Service-Integration regressionssicher prüfen.
2. F03 vor Freischaltung der dadurch erreichbaren Ansicht beheben.
3. F04, F05 und F06 für vollständige und konsistente Kontaktinformationen korrigieren.
4. F07 bis F10 für verlässliche Fehlermeldungen, Refresh-/Suchzustände und Tastaturbedienung korrigieren.
5. F11 mit dem vorhandenen Ressourcenbudget-Muster angleichen.
6. Abschließend den vorgesehenen Linux-/Omarchy- und Standalone-UI-Pfad mit synthetischen Kontakten prüfen; reine View-Mocks reichen dafür nicht.

Diese Reihenfolge ist eine Empfehlung für spätere Arbeiten. Der Audit-Commit fügt ausschließlich diesen Bericht hinzu und enthält keine Implementierungs-, Test-, Konfigurations- oder Releaseänderungen.
