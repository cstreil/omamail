# Native CardDAV address books

This document is the working plan for the experimental CardDAV branch. It is
not an upstream commitment and does not describe a released capability.

## Goal

Give Omamail one shared address-book model that can discover CardDAV address
books, cache contacts locally, edit them, and synchronize changes safely. The
existing recipient suggestions should consume that model alongside their local
mail-cache fallback instead of implementing a second contact path.

The implementation must remain provider-neutral. Stalwart is the first live
server used for development, not a special-case provider.

## Current state

`contacts.suggest` is deliberately read-only. The Rust contact module harvests
name and email pairs from Thunderbird/Betterbird databases, Omamail's mail
cache, and optional `contacts.json` / `contacts.vcf` files. The QML contact
picker can search and choose a recipient but cannot display an address-book
source or edit a contact.

Calendar code already establishes useful DAV precedents: HTTPS-only configured
origins, credentials loaded only after validating the destination, bounded
requests and responses, no redirects carrying credentials, native Secret
Service/Keychain/Credential Manager storage, private atomic caches, and QML
presentation over a versioned backend API.

## First vertical slice

The first useful slice is intentionally smaller than complete offline sync:

1. Configure or discover one or more CardDAV address-book collections.
2. List and read vCard 3.0/4.0 contacts through the Rust backend.
3. Cache a normalized contact projection and feed it to recipient suggestions.
4. Create, update, and delete a contact online with ETag preconditions.
5. Present conflicts explicitly; never overwrite a changed server resource.
6. Provide a contact list, search, detail view, and a minimal editor for name,
   email addresses, telephone numbers, and postal addresses.

The first slice does not promise contact groups, photos, CardDAV push, arbitrary
vendor extensions, or an offline mutation queue. Unknown vCard properties must
survive a read-modify-write round trip even when the UI cannot edit them.

## Backend boundary

CardDAV networking belongs in Rust. Proposed API-6 methods are grouped below;
exact request and response shapes must be fixed in `backend-api.json` before QML
depends on them.

- `contacts.sources`: configured and discovered address books
- `contacts.sync`: bounded collection synchronization
- `contacts.list`: cached contact summaries
- `contacts.get`: one complete normalized contact plus opaque source metadata
- `contacts.create`: conditional collection write
- `contacts.update`: conditional resource write using the known ETag
- `contacts.delete`: conditional resource delete using the known ETag

The source registry, credentials, cache and pending conflict state remain native
backend concerns. QML owns navigation, selection, forms, validation feedback,
and optimistic presentation only where rollback is complete and tested.

## Synchronization rules

- Use collection `sync-token` when advertised; otherwise compare a bounded
  depth-one multistatus snapshot of hrefs and ETags.
- Treat href as opaque server identity and keep it on the configured origin.
- Send `If-Match` for updates and deletes and `If-None-Match: *` for creates.
- A failed precondition becomes a visible conflict, never last-writer-wins.
- Apply a successful remote snapshot and cache replacement atomically.
- Preserve unknown vCard properties and their parameters across edits.
- Keep the existing local harvesters as a separate suggestion source; they do
  not become server contacts unless the user explicitly creates one.

## Security invariants

- Credentials never enter argv, logs, settings, diagnostics, QML, or returned
  RPC values.
- Credentials are loaded only after the configured HTTPS origin and request
  destination have passed validation.
- Redirects are refused for authenticated DAV requests.
- Every request has an overall deadline and bounded response body.
- XML, hrefs, ETags, display names and vCards are untrusted server input.
- Tests must prove that malformed or foreign-origin resources cause no network
  request, credential lookup, cache write, or destructive fallback.

## Verification plan

- Rust unit tests for DAV discovery, multistatus parsing, sync-token fallback,
  vCard normalization and lossless preservation, ETag conflicts, bounds and
  credential-origin rules.
- Backend contract fixtures for every new method and stable error.
- QML tests for navigation, source selection, search, validation, editor state,
  conflict presentation and recipient-picker reuse.
- Standalone composition tests because the feature is shared by both hosts.
- `LC_ALL=C make validate`, `cargo test --locked --lib`,
  `make test-backend-process`, and the standalone Linux gate on the final
  implementation commit. The explicit locale keeps the release-script sort
  fixture deterministic on this development machine.
- Live round trips against a dedicated test address book: server to client,
  client to server, concurrent update conflict, delete, restart and offline
  recovery. No personal contact data belongs in fixtures, screenshots or logs.
- Manual UI checks in light and dark themes, full and compact layouts, with
  synthetic contacts and matching before/after screenshots.

## Delivery sequence

1. Read-only discovery, cache and recipient suggestions.
2. Contact view and source management.
3. Online create/update/delete with ETag conflict handling.
4. Incremental synchronization and offline mutation design.
5. Fresh-agent security and behavior review, full validation, live evidence,
   then decide with the maintainer whether to prepare an upstream pull request.
