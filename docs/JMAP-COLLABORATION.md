# Native JMAP contacts and calendars

This document is the working plan for the experimental JMAP collaboration
branch. It is not an upstream commitment and does not describe a released
capability.

## Goal

Give Omamail one shared contact model and extend its existing calendar model so
a JMAP account can provide mail, contacts and calendars through the same
session, account identity and credential. The public backend API and QML remain
protocol-neutral: JMAP is the first provider, not a UI-level data model.

The initial live server is Stalwart. Implementation decisions must follow the
advertised JMAP capabilities and standards rather than depend on Stalwart-only
fields or URLs.

## Verified development target

The development account advertises all three account capabilities through the
credential already stored for Omamail:

- `urn:ietf:params:jmap:mail`
- `urn:ietf:params:jmap:contacts`
- `urn:ietf:params:jmap:calendars`

`AddressBook/get` and `ContactCard/query` have been verified without exposing
contact data. A separate address book containing one synthetic JSContact/vCard
fixture is reserved for destructive live tests. Personal address books are not
test fixtures.

JMAP for Contacts is RFC 9610. JMAP for Calendars remains an IETF draft, so the
calendar adapter must keep draft-version assumptions isolated from the shared UI
and cache model.

## Current Omamail state

`contacts.suggest` is deliberately read-only. The Rust contact module harvests
name and email pairs from Thunderbird/Betterbird databases, Omamail's mail
cache, and optional `contacts.json` / `contacts.vcf` files. The QML picker can
search and choose a recipient but cannot display an address-book source or edit
a contact.

The JMAP mail provider already owns authenticated session discovery, capability
checks, bounded native HTTPS requests, account identity, query/get/set patterns,
and change handling. The first contact implementation should extend that native
provider rather than introduce a second JMAP client or expose tokens to QML.

Calendar UI currently normalizes Google, Microsoft and CalDAV results. JMAP
calendar support should become another backend provider feeding that shared
calendar projection; the UI must not parse JSCalendar directly.

## Delivery sequence

### 1. Read-only JMAP contacts

1. Extend the existing protocol-neutral `contacts.suggest` call with an optional
   account id while preserving `{}` as the released local-only behavior.
2. For a JMAP account, select its readable default address book and fetch a
   bounded `ContactCard` projection through the existing native JMAP session.
3. Merge normalized name/email rows with the existing local harvesters and use
   them immediately in the composer without exposing JMAP values to QML.
4. Gate the account-scoped request on backend API 6 and prevent late responses
   or cached suggestions from crossing account boundaries.
5. Add explicit source/list/detail methods only with the address-book UI, when
   their pagination and identity semantics have a real consumer.

This first slice is deliberately on-demand and read-only. Persistent remote
caching, `ContactCard/changes`, source selection and mutations belong to later
slices rather than being implied by recipient suggestions.

### 2. Native address-book UI

1. Add a contact list with source selection, search and stable contact identity.
2. Add a detail view for names, email addresses, telephone numbers and postal
   addresses.
3. Reuse the same normalized model in the composer recipient picker.
4. Preserve full JSContact data needed for a lossless update even when the UI
   cannot edit every field.

### 3. Contact mutations and synchronization

1. Create, update and destroy `ContactCard` objects through JMAP `set` methods.
2. Use `ifInState` on mutations and present `stateMismatch` as an explicit
   conflict; never silently overwrite a changed server object.
3. Use `ContactCard/changes` and query state for incremental synchronization.
4. Queue offline mutations only after their persistence, ordering, cancellation
   and conflict semantics are separately designed and tested.

### 4. JMAP calendars

1. Project `Calendar/get` results into the existing calendar source model.
2. Adapt `CalendarEvent/query`, `get` and `set` to the shared event projection.
3. Keep JSCalendar recurrence, time-zone and scheduling semantics in Rust.
4. Prefer JMAP automatically for a signed-in JMAP account that advertises the
   calendar capability; retain existing providers and manual DAV sources.

## Backend boundary

The UI-facing methods use product concepts rather than protocol names. API 6
extends the existing `contacts.suggest` method with the optional shape
`{"accountId":"..."}`; the released `{}` request remains local-only and unchanged.
The same unreleased revision carries the read-only address-book surface the
contact view needs:

- `contacts.suggest`: optional `{"accountId"}`; local plus remote suggestions
- `contacts.sources`: `{"accountId"}` -> `{sources:[{id,name,default,subscribed,readOnly}]}`
- `contacts.list`: `{"accountId","source","query"?,"limit"?,"position"?}` ->
  `{contacts:[{id,name,emails,source}],total,position}`; 50 rows by default,
  200 at most, and ordering comes from the backend because the server supports
  no contact sort
- `contacts.get`: `{"accountId","id"}` ->
  `{contact:{id,source,name,emails,phones,addresses}}` with labeled entries

Accounts without the JMAP contacts capability answer `contacts.sources` and
`contacts.list` with an empty result rather than an error, so a mixed account
list needs no provider branching in QML. Unknown accounts, unknown sources and
unknown ids have their own stable errors.

Not implemented yet; these need their own revision and a real consumer:

- `contacts.create`: create a contact in a writable source
- `contacts.update`: update with the last known JMAP state
- `contacts.delete`: destroy with the last known JMAP state
- `contacts.sync`: request bounded incremental synchronization

JMAP-specific method names, `using` capabilities, request batching, state tokens
and raw ContactCard values remain implementation details below this boundary.
The source registry, credentials, cache and pending conflict state are backend
concerns. QML owns navigation, selection, forms and validation feedback.

## Contact projection

The first normalized contact shape needs stable semantics rather than mirroring
one protocol object:

- stable composite identity: account, source and contact id
- display name and structured name components
- zero or more labeled email addresses
- zero or more labeled telephone numbers
- zero or more labeled postal addresses
- source id, read/write rights and current synchronization state
- an opaque backend-owned revision reference for safe mutations

Recipient suggestions remain a smaller projection of this model. Local harvested
addresses have synthetic local identities and cannot be edited or uploaded until
the user explicitly creates a server contact.

## Security invariants

- Credentials never enter argv, logs, settings, diagnostics, QML, cache values,
  or returned RPC data.
- The existing JMAP session URL, API URL and download/upload origins remain the
  sole trusted destinations established by session discovery.
- Account and capability checks complete before any contacts or calendar call.
- Every request has an overall deadline, bounded body and bounded object count.
- JSON values, ids, state tokens, names and contact fields are untrusted input.
- Malformed input and capability refusal cause no mutation or cache replacement.
- Errors returned to QML are stable codes and never include server bodies,
  credentials or personal contact data.

## Verification plan

- Rust unit tests for capability refusal, request construction, pagination,
  normalization, bounds, state mismatch and credential/session isolation.
- Backend contract fixtures for every new method and stable error.
- Golden parity tests proving the composer receives the same suggestion shape
  from local and JMAP contact projections.
- QML tests for navigation, source selection, search, validation, editor state,
  conflict presentation and recipient-picker reuse.
- Standalone composition tests because the feature is shared by both hosts.
- `LC_ALL=C make validate`, `cargo test --locked --lib`,
  `make test-backend-process`, and the standalone Linux gate on the final
  implementation commit. The explicit locale keeps the release-script sort
  fixture deterministic on this development machine.
- Live round trips only against the isolated synthetic address book: server to
  client, client to server, concurrent state conflict, delete, restart and
  offline recovery. No personal contact data belongs in fixtures, screenshots
  or logs.
- Manual UI checks in light and dark themes, full and compact layouts, with
  synthetic contacts and matching before/after screenshots.

## Upstream boundary

The experimental branch may optimize the order of implementation for a complete
JMAP account, but the resulting code must not describe Stalwart as the provider.
Before proposing an upstream pull request, split the work into reviewable
vertical changes, obtain a fresh-agent review, and decide with the maintainer
whether JMAP contacts, the address-book UI and JMAP calendars should be separate
pull requests.
