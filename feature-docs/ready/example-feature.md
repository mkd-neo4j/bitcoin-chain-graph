---
title: User Authentication
status: ready
priority: high
ideation-ref: feature-docs/ideation/user-auth/
affected-files:
  - src/auth/authenticate.ts
  - src/auth/session.ts
  - src/stores/auth-store.ts
  - src/components/login-form.tsx
---

# User Authentication

## Summary

Add email/password login with session management. Users can log in, stay
authenticated across page reloads, and log out. This is the foundation for
all protected routes and role-based access control. The auth store follows
the existing tab-isolated store pattern in `src/stores/`.

## Acceptance Criteria

1. GIVEN a valid email and password WHEN `authenticate(email, password)` is called
   THEN it returns a `Session` object with a non-null `token` string and an
   `expiresAt` timestamp at least 1 hour in the future
2. GIVEN an email that does not match any user WHEN `authenticate(email, password)`
   is called THEN it throws `AuthenticationError` with code `"INVALID_CREDENTIALS"`
3. GIVEN a correct email but wrong password WHEN `authenticate(email, password)` is
   called THEN it throws the same `AuthenticationError` with code
   `"INVALID_CREDENTIALS"` (no distinction between bad email and bad password)
4. GIVEN a successful authentication WHEN the `Session` is returned THEN
   `authStore.getState().session` contains the `Session` object and
   `authStore.getState().isAuthenticated` is `true`
5. GIVEN `authStore.getState().isAuthenticated` is `true` WHEN `logout()` is called
   THEN `authStore.getState().session` is `null`, `isAuthenticated` is `false`,
   and the session cookie is cleared via `document.cookie`
6. GIVEN a session cookie exists with a valid token WHEN the app initializes and
   calls `restoreSession()` THEN `authStore.getState().session` is populated from
   the cookie and `isAuthenticated` is `true`
7. GIVEN a session cookie exists with an expired token WHEN `restoreSession()` is
   called THEN `authStore.getState().session` is `null`, `isAuthenticated` is
   `false`, and the cookie is cleared

## Edge Cases

- Empty string for email or password passed to `authenticate()` — throws
  `ValidationError` with code `"EMPTY_FIELD"` before making any network request
- Network timeout during authentication (request takes >5s) — `authenticate()`
  throws `NetworkError` with code `"TIMEOUT"`; the login form shows the error and
  preserves the email field value but clears the password field
- `authenticate()` called while a previous call is still pending — second call is
  rejected with `AuthenticationError` code `"REQUEST_IN_FLIGHT"` (debounce at the
  store level, not the UI level)
- Session cookie contains malformed JSON — `restoreSession()` clears the cookie
  and sets `isAuthenticated` to `false` without throwing

## Out of Scope

- **OAuth/social login** — separate feature; do NOT add any OAuth types to the
  `Session` interface or the auth store even as optional fields
- **Password reset flow** — separate feature (depends on email service integration)
- **Server-side rate limiting** — backend concern outside this feature's
  `affected-files`
- Do NOT refactor the existing `src/api/client.ts` request interceptor to add auth
  headers — that will be a follow-up feature. The interceptor currently has a
  `TODO: add auth` comment; leave it as-is. Touching `client.ts` risks breaking
  all existing API calls and it is not in `affected-files`

## Technical Notes

- Follow the existing store pattern in `src/stores/tab-store.ts` for the auth
  store structure (actions, selectors, middleware)
- Session token storage uses an httpOnly cookie, not localStorage. The
  `document.cookie` API is used for clearing only; writing is handled by the
  server's `Set-Cookie` response header
- Use the existing `api.post()` from `src/api/client.ts` for the login request —
  do not create a separate fetch call
- **Rejected**: Storing the session in localStorage with an encryption wrapper.
  Rejected during ideation because localStorage is accessible to XSS and
  encryption in JS provides no meaningful protection against a script running in
  the same origin. httpOnly cookies are invisible to JavaScript entirely.

## Style Requirements (frontend only)

- Login form uses the existing `Card` + `Form` composition from shadcn/ui
- Error messages use the `Alert` component with `variant="destructive"`
- Loading state renders a `Loader2` spinner inside the submit `Button` with
  `disabled={true}` — do not add a separate loading overlay
- Screenshot baselines needed: yes (login form default, error state, loading state)
