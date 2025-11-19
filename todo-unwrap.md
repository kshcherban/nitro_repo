# TODO — Unwrap/Expect Remediation Plan

## 1. Repository HTTP surface (`nitro_repo/src/repository/repo_http.rs`)
- Replace remaining `Response::builder().body(...).unwrap()` usages (lines ~630, ~700) with the shared `ResponseBuilder` helpers we introduced.
- Extract additional helpers for directory listings / auth failures to avoid duplication.
- Ensure Docker-specific responses reuse the new constructors and add unit tests covering `www_authenticate`, forbidden, and method-not-allowed cases.
- Tests: `cargo test -p nitro_repo docker_v2_ok_response_sets_headers docker_v2_unauthorized_response_sets_challenge` plus any new ones we add.

## 2. Debian metadata writers (`nitro_repo/src/repository/deb/metadata.rs`)
- Replace `writeln!(...).unwrap()` sequences by writing through safe helpers (e.g., `push_label_value`) for release files as well as packages entries.
- Keep formatting identical; extend existing tests (`packages_entry_contains_required_fields`, `release_file_lists_all_hashes`) to validate the refactor.
- Tests: `cargo test -p nitro_repo packages_entry_contains_required_fields release_file_lists_all_hashes`.

## 3. User HTTP APIs (`nitro_repo/src/app/api/user*.rs` & `user_management.rs`)
- Convert every manual `Response::builder().body(...).unwrap()` to `ResponseBuilder`, especially in `user.rs`, `user_management.rs`, `tokens.rs`, `password_reset.rs`, and `mod.rs`.
- Ensure session/login flows keep the exact headers (cookies, set-cookie) by adding focused unit tests where missing.
- Tests: `cargo test -p nitro_repo` (targeted modules), plus existing http unit suites.

## 4. Repository docker backend (`nitro_repo/src/repository/docker/{auth,handlers,mod}.rs`)
- Audit for `.unwrap()`/`.expect()` when building responses, serializing JSON, or handling hashing.
- Introduce helper functions (similar to `docker_v2_ok_response`) for bearer challenges, pagination responses, and manifest builders.
- Add regression tests for `docker::handlers` to cover `parse_pagination_params`, `finalize_upload`, etc., so we can safely remove the unwraps.
- Tests: `cargo test -p nitro_repo docker::handlers::tests::*` (existing ones) and any new coverage we add.

## 5. Repository Deb metadata writers & release builders
- After helper extraction, ensure `ReleaseEntry` formatting and Packages builder no longer panic; confirm multi-line description formatting survives.
- Consider adding snapshot tests (string equality) to lock the formatting.

## 6. HashSet disallowance cleanup (`app/api/repository/packages.rs`, `repository/docker/auth.rs`, etc.)
- Replace `std::collections::HashSet` with `ahash::HashSet` per `clippy::disallowed_types` warnings while refactoring unwraps.
- Touch the same areas when swapping `.unwrap()` (e.g., `HashSet::new()` sites) to keep churn localized.
- Tests: `cargo clippy --workspace` to verify the lint is satisfied once conversions are complete.

## 7. Final verification
- Run `cargo fmt`, `cargo test --workspace`, and `cargo clippy --workspace`.
- Document the effort by updating `history/MEMORY.md` once clippy passes without `unwrap_used`/`expect_used` violations.
