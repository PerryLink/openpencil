# Hosted models

The native Hosted runtime shared by desktop, the daemon, Android and iOS.
`op-host-services::hosted_models` re-exports this crate for existing desktop
callers. The HTTP daemon's request parser remains in `op-host-services`.

Hosts call `sync_editor` after polling account state and continue frame polling
while `needs_poll` is true. A selected opaque route is resolved with
`provider_for_route`; no saved provider credential or user-supplied endpoint is
accepted. Providers carry an auth session epoch, and hosts must check
`session_epoch_is_current` before applying buffered output. Logout also calls
`clear` and clears the editor's Hosted catalog.

The runtime uses the same `op-chat-agent` executor and cancellable receiver as
other native chat providers. It introduces no daemon, CLI, process-spawn or GL
dependency. Private access requests are cancelled when their handles drop;
catalog and paid requests reject a changed identity or stale route. Requests
are never automatically replayed after a transport failure.

Hosted Billing currently excludes vision from its billable capability set.
Accordingly this transport keeps `image_input` disabled and rejects attachments
before requesting private authorization or contacting the gateway. Hosts must
keep the draft and attachments when showing that error. Enabling images later
requires a supported billing contract, catalog capability and tested wire
format together; an upstream model's vision support alone is insufficient.

Run `cargo test -p op-hosted-models --lib` for catalog, session, cancellation,
protocol, attachment rejection and bounded streaming tests. The shared
receiver's cancellation/cleanup tests run in `op-chat-agent`.
