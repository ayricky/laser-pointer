# Submit Laser Pointer to the Microsoft Store

Prepared 19 October 2026. This checklist does not register an account, publish the site, install
the app or submit a package. Partner Center labels and requirements can change; its live form
and validation take precedence. See [listing.md](listing.md) for the prepared copy and artwork plan.

## 1. Create a developer account

- [ ] Start at [Microsoft's developer-account guide](https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/open-a-developer-account)
  and sign into Partner Center with the intended owner account. Choose Individual for publishing
  as Ricardo Mariano, or Company only for a registered business.
- [ ] Complete the current identity verification, profile, agreements and contact details. New
  onboarding currently advertises free registration; confirm availability and any charges in your
  market rather than relying on older US$19/US$99 fee guidance.
- [ ] Complete the tax and payout setup required for paid sales. Keep legal identity information
  consistent with the account; do not post it in this repository.

## 2. Reserve the name and record the identity

- [ ] Create a new app and reserve **Laser Pointer**. Use a fallback from [listing.md](listing.md)
  if unavailable. Select the **MSIX package** submission path, rather than an externally hosted
  MSI/EXE installer listing.
- [ ] Open the app's **Product identity** page (often under Product management). Copy exactly:
  **Package/Identity/Name**, **Package/Identity/Publisher**, and **PublisherDisplayName**.
  These are package identity values, not the public Store product ID.
- [ ] Keep these values for every future update. The default `RicardoMariano.LaserPointer` and
  `CN=LaserPointerDev` values in the script are development placeholders; do not submit them unless
  they are literally the values assigned by Partner Center.

## 3. Build and inspect the package

- [ ] Install Rust stable/MSVC, Visual Studio Build Tools and the Windows 10/11 SDK on a build PC.
  The SDK must include `rc.exe`, `makepri.exe`, `makeappx.exe` and, for optional signing, `signtool.exe`.
  CI also creates a development-identity package, which must be rebuilt with the Store identity.
- [ ] From the repository root, use the exact values copied above:

```powershell
./packaging/build-msix.ps1 `
  -IdentityName 'PASTE_PACKAGE_IDENTITY_NAME' `
  -Publisher 'PASTE_PACKAGE_IDENTITY_PUBLISHER' `
  -PublisherDisplayName 'PASTE_PUBLISHER_DISPLAY_NAME' `
  -TargetDir target-codex
```

The default package version comes from Cargo.toml: `1.1.0` becomes `1.1.0.0`. For subsequent
submissions increase the version; keep the fourth part zero for the Store. `-Version 1.1.1.0`
overrides the package version. Bump Cargo.toml too when releasing so executable metadata agrees.
`-NoBuild` uses an existing release executable from the chosen target directory.

- [ ] Confirm `dist/LaserPointer_1.1.0.0_x64.msix` exists. The script stages the executable,
  licences, qualified artwork, manifest and resource index under `dist/msix/stage` and runs SDK
  package validation while packing. Inspect the staged identity for typos.
- [ ] Unpack into a fresh directory with the SDK's `makeappx unpack /p <package> /d <directory>`;
  confirm `resources.pri`, Assets, both licence files and the executable are present.
- [ ] On a separate test PC, check installation/uninstallation, mixed-DPI/multiple monitors,
  light/dark theme, shortcuts, undo/redo/Clear, first launch, tray behaviour and Windows startup.
  The startup task is `LaserPointerStartup`, initially disabled. Enable it from the app, sign out
  and in, and verify Windows Settings/Task Manager can disable it.
- [ ] Run the Windows App Certification Kit on the test PC before submission. Packaging validation
  alone does not verify runtime behaviour or certification compliance.

Leave Store submissions **unsigned**; Microsoft signs them on ingestion. For optional developer
sideload testing, use an existing PFX whose subject matches `-Publisher`:

```powershell
$password = Read-Host 'PFX password' -AsSecureString
./packaging/build-msix.ps1 -TargetDir target-codex -NoBuild `
  -CertificatePath 'C:\private\developer.pfx' -CertificatePassword $password
```

Certificate trust and sideload installation are separate test-PC steps. Do not add certificates
or install packages on an actively used PC as part of building. Signing is optional and has not
been verified here without a suitable certificate.

## 4. Create the submission and upload

- [ ] Create a submission, upload the `.msix` under Packages, and resolve identity, architecture,
  version or manifest errors. Do not upload the zip as the Store package.
- [ ] Confirm the detected family is Windows.Desktop, minimum OS 10.0.17763.0, x64. Check the
  product name and en-US language, select Productivity and mouse/keyboard recommendations.
- [ ] Complete any export/compliance or accessibility declarations based on the actual release.
  Do not claim accessibility features that have not been tested.

## 5. Explain runFullTrust

The package declares the restricted `runFullTrust` capability because the executable is a
classic desktop app running outside AppContainer. Partner Center requests a justification
for restricted capabilities, generally under Submission options; approval is Microsoft's decision.
See [capability declarations](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations).

Ready to paste:

```text
Laser Pointer is a native Rust/Win32 presentation utility. It needs to run as a full-trust desktop application to create a transparent screen-wide annotation overlay across desktop applications, render through Direct2D/DirectComposition, register user-configurable global hotkeys, display a notification-area icon, and pass user-requested clicks and scrolling to the application underneath. These desktop integrations are central to its purpose and are not available to this implementation inside AppContainer.

The application runs at the signed-in user's normal privilege level and does not request elevation. It has no network connections, telemetry, advertising, accounts or personal data collection. Settings are stored locally. Optional start with Windows uses the declared LaserPointerStartup task, initially disabled and controlled by the user. Source code is available under the MIT licence at https://github.com/ayricky/laser-pointer.
```

If the live portal asks for an additional capability review, provide it there. Do not remove
`runFullTrust` to bypass review; it is required by this application model.

## 6. Set pricing, availability and ratings

- [ ] Under Pricing and availability, select the live USD tier corresponding to **US$2.99**.
  Review converted prices and markets. Leave trials disabled initially; a time-limited trial needs
  licensing behaviour verified in the app first. No subscriptions or in-app products are needed.
  [Current MSIX pricing guidance](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/price-and-availability).
- [ ] Choose public availability, markets and a release schedule appropriate for launch. Check
  that paid-app payout/tax setup is complete and the final price has not defaulted to Free.
- [ ] Complete the age-rating questionnaire using [listing.md](listing.md). There is no online
  sharing, in-app purchasing, advertising or data collection. The initial Store payment is not an
  in-app purchase. The questionnaire assigns ratings; do not assume one in advance.

## 7. Fill the listing and publish the web pages

- [ ] Add the prepared English text and links from [listing.md](listing.md). For a first Store
  submission leave “What's new” blank if the portal follows Microsoft's current guidance.
- [ ] Capture at least one real desktop screenshot on a test PC; four are planned in the kit.
  No screenshots are supplied. Upload the 300 × 300 app icon from
  `packaging/Assets/Square150x150Logo.scale-200.png` if using a custom listing icon.
- [ ] Ensure the approved repository changes have reached `main` through your normal review process.
  In GitHub Settings → Pages choose **Deploy from a branch**, `main`, `/docs`. No Pages workflow
  or build step is required; `.nojekyll` serves the static files directly.
- [ ] Verify the public website, privacy policy and support links load. Ensure the separately
  authored `docs/whats-new.html` is present before launch. The policy takes effect 19 October 2026.
- [ ] Once the product URL is available, replace `https://apps.microsoft.com/` in the website's
  `#store-link` and README with the product URL. Confirm the displayed US price matches the listing.

## 8. Provide certification notes and submit

Ready-to-paste tester notes:

```text
Laser Pointer lives in the notification area; there is no main document window. If necessary, expand the hidden notification-area icons. Left-click its icon to open settings. Press Alt+L to turn the laser on, then drag over the desktop or another application to draw. Esc turns it off. Alt+Shift+L quits.

Select Pen or turn on Keep strokes to retain annotations. Ctrl+Z undoes, Ctrl+Y redoes (both while the laser is on), and Alt+X clears visible strokes. Undo immediately after Clear restores the cleared drawings. Right-click passes a click through; the wheel scrolls underneath and Ctrl+wheel adjusts thickness.

No account, login, network connection or test credentials are needed. Start with Windows is off initially and can be enabled in Options; Windows also controls this startup permission. Launching again opens the running copy's settings. The app runs without administrator rights.
```

- [ ] Review every section and portal warning, package identity, pricing, ratings, privacy URL,
  screenshots, capability justification and certification notes.
- [ ] Submit for certification. Monitor Partner Center for requests and respond with reproducible
  steps if a tray-only launch is mistaken for an application that failed to start.
- [ ] After approval, check the actual Store listing, installation, automatic updates and purchase
  flow on a separate test machine. Certification, Store signing and updates cannot be confirmed by
  an unsigned local pack/unpack check.
