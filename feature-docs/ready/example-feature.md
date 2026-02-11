---
title: User Authentication
status: ready
priority: high
ideation-ref: feature-docs/ideation/user-auth/
affected-files:
  - src/auth/login.ts
  - src/auth/session.ts
  - src/stores/auth-store.ts
  - src/components/login-form.tsx
---

# User Authentication

## Summary

Add email/password login with session management. Users can log in, stay
authenticated across page reloads, and log out. This is the foundation for
all protected routes and role-based access control.

## Acceptance Criteria

1. GIVEN a valid email and password WHEN the user submits the login form
   THEN a session token is stored and the user is redirected to the dashboard
2. GIVEN an invalid email or password WHEN the user submits the login form
   THEN an error message is displayed and the form is not cleared
3. GIVEN a logged-in user WHEN they reload the page
   THEN they remain authenticated (session persists)
4. GIVEN a logged-in user WHEN they click logout
   THEN the session is cleared and they are redirected to the login page
5. GIVEN an expired session token WHEN any authenticated request is made
   THEN the user is redirected to the login page with an expiry message

## Edge Cases

- Empty email or password fields — show validation errors before submission
- Network failure during login — show a connection error, allow retry
- Multiple rapid login attempts — debounce to prevent duplicate requests
- Session token missing from storage — treat as logged out, redirect to login

## Out of Scope

- OAuth/social login (separate feature)
- Password reset flow (separate feature)
- Rate limiting (backend concern)
- Multi-factor authentication (future feature)

## Technical Notes

- Follow the existing store pattern in `src/stores/` for the auth store
- Session token goes in httpOnly cookie (not localStorage)
- Use the existing `api` utility for HTTP requests
- Error messages should use the project's i18n system if one exists

## Style Requirements (frontend only)

- Login form matches existing shadcn Card + Form patterns
- Error messages use destructive variant of Alert component
- Loading state shows spinner inside the submit button
- Screenshot baselines needed: yes (login form, error state, loading state)
