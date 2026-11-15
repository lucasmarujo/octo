# Automated release: Google Play and App Store

Workflow: [`.github/workflows/mobile-release.yml`](../workflows/mobile-release.yml).

## Flow

| Event | Android | iOS |
|---|---|---|
| push to `main` touching `mobile/` | signed AAB → Google Play internal testing | EAS build → TestFlight |
| tag `mobile-vX.Y.Z` | signed AAB → Google Play production, with release notes, and a GitHub release | EAS build → TestFlight → submitted for App Store review (released automatically once approved) |
| pull request touching the pipeline | workflow lint, script tests and a signed AAB as an artifact, nothing published | — |
| `workflow_dispatch` | signed AAB as an artifact; publishes to internal testing only with `publish` checked | same, TestFlight |

The app CI (`tsc` and `jest` in `mobile/`) runs before any build. If it fails, nothing is published.

## Version

- **versionName / version**: `expo.version` in `mobile/app.json`. The tag must match (`mobile-v1.0.2`), or the workflow fails
  before building. Flow: raise the version, merge, create the tag.
- **versionCode (Android)**: `1000 + run number` of this workflow, applied at build time by
  `android-version-code.gradle`. No manual edits. Do not rename the workflow: the run number restarts.
- **buildNumber (iOS)**: EAS increments it on its servers (`appVersionSource: remote` + `autoIncrement` in `eas.json`).
- **Notes**: the message of an annotated tag (`git tag -a mobile-v1.0.2 -m "..."`); without one, the
  commits since the previous tag (up to 500 characters). Listing language: `en-US`.

## Secrets

| Secret | Content | Status on 2026-11-14 |
|---|---|---|
| `ANDROID_KEYSTORE_BASE64` | base64 upload keystore | set |
| `ANDROID_KEYSTORE_PASSWORD` | keystore password | set |
| `ANDROID_KEY_ALIAS` | key alias | set |
| `ANDROID_KEY_PASSWORD` | key password | set |
| `PLAY_SERVICE_ACCOUNT_JSON` | service account JSON with release permission on the app (Play Console) | **missing** |
| `EXPO_TOKEN` | access token of the Expo account that owns the project | **missing** |
| `APP_STORE_CONNECT_API_KEY_BASE64` | base64 App Store Connect API `.p8` key | set |
| `APP_STORE_CONNECT_API_KEY_ID` | key ID | set |
| `APP_STORE_CONNECT_API_ISSUER_ID` | team issuer ID | set |
| `GOOGLE_SERVICES_JSON_BASE64` | base64 `google-services.json` (Firebase, push notifications) | set |
| `IOS_CREDENTIALS_BASE64` | base64 `.tar.gz` with `credentials.json` (the `ios` part, relative paths) and the `.p12`/`.mobileprovision` | set |

Without `PLAY_SERVICE_ACCOUNT_JSON` the AAB is built and kept as an artifact, with a warning. Without the
iOS secrets the iOS job is skipped, with a warning.

## Release

```bash
# internal testing and TestFlight: merging into main is enough
git tag -a mobile-v1.0.2 -m "What changed in this version"
git push origin mobile-v1.0.2
```

Mobile tags use the `mobile-v` prefix because `vX.Y.Z` tags belong to the desktop releases, and the mobile
GitHub release is created with `--latest=false`, so `releases/latest` keeps pointing at the desktop.

`PLAY_RELEASE_STATUS` is `draft`, as `releaseStatus` in `eas.json`, because Google Play only accepts draft
releases while the app itself is a draft. Switch it to `completed` once the app is published.

The Android build does not use EAS: it runs `expo prebuild` and Gradle on the runner. Stop sending manual EAS
builds to the store: their versionCode is lower than the CI one and Google Play would reject them.
