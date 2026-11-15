# Signing the releases

## Where it stands

| Platform | Today | What people see |
| --- | --- | --- |
| macOS | ad-hoc signed (whole app, hardened runtime); not notarized | "Apple could not verify SketchCraft": **System Settings › Privacy & Security › Open Anyway** |
| Windows | unsigned | SmartScreen: **More info › Run anyway** |
| Linux | no signing needed | nothing |

## macOS: Developer ID and notarization

The release workflow signs and notarizes automatically once these
repository secrets exist (Settings › Secrets and variables › Actions).
Without them it builds exactly as today.

1. Join the Apple Developer Program (99 USD a year) at developer.apple.com.
2. In Xcode or the developer site, create a **Developer ID Application**
   certificate and export it from Keychain Access as a `.p12` with a password.
3. Add the secrets:

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | the `.p12`, base64: `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12`'s password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` (`security find-identity -v -p codesigning`) |
| `APPLE_ID` | your Apple ID email |
| `APPLE_PASSWORD` | an **app-specific password** from appleid.apple.com (not your Apple ID password) |
| `APPLE_TEAM_ID` | the 10-character team id |

4. Push a new tag. The DMG and app are then signed with your Developer ID
   and notarized, and open with no warning.

## Windows

Options, cheapest first:

- **SignPath Foundation**: free code signing for open-source projects
  (apply at signpath.org; they sign builds from CI).
- **Azure Trusted Signing**: about 10 USD a month; needs identity
  validation. Tauri can call it through `bundle.windows.signCommand`.
- An OV or EV certificate from a certificate authority.

Each needs its own small workflow change; tell Claude which you pick.
