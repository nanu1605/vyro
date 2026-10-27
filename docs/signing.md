# Code signing (2.0)

Windows' SmartScreen warns about an installer from an unknown publisher until enough people have
installed it. A signed installer from a known certificate does not get that warning (or loses it
quickly), and Windows names the publisher in the install prompt. Decision 11 picked a free
certificate for open-source projects: **SignPath Foundation** (or Certum Open Source).

The release workflow (`.github/workflows/release.yml`) is ready: once four settings are in the
repository, every release signs `vyro.exe` and `vyro-service.exe` before they go into the MSI,
then signs the MSI, then makes the updater's signature for the signed MSI (the updater checks
that signature before it installs anything). Without the settings, releases build unsigned, as
before.

## What only the owner can do

SignPath Foundation signs only open-source projects whose builds it can trace to their source:

1. **Make the repository public.** It is already licensed MIT OR Apache-2.0 (`LICENSE-MIT`,
   `LICENSE-APACHE`), both OSI licences. Look through the history first: anything in it becomes
   public too. The releases repository (`RELEASES_REPO`) can then go: installed copies update
   from wherever `latest.json` says, so keep it until every installed copy has moved to a
   version that updates from this repository (or keep it for good; nothing requires removing it).
2. **Apply at <https://signpath.org/apply>** with the repository's address. They check the
   project (a real, maintained open-source project with a release history) and that the build
   runs on GitHub's own runners from a workflow in the repository. Approval takes a few weeks.
3. **In SignPath, once approved:**
   - the project's slug (use `vyro`) and a signing policy (`release-signing`);
   - the GitHub integration: the trusted build system "GitHub.com", linked to this repository;
   - two artifact configurations (below), with the slugs `programs` and `installer`;
   - a CI user, and its API token.
4. **In this repository** (Settings → Secrets and variables → Actions):

   | Kind     | Name                          | Value                              |
   |----------|-------------------------------|------------------------------------|
   | variable | `SIGNPATH_ORGANIZATION_ID`    | the organization's id in SignPath  |
   | variable | `SIGNPATH_PROJECT_SLUG`       | `vyro`                             |
   | variable | `SIGNPATH_SIGNING_POLICY_SLUG`| `release-signing`                  |
   | secret   | `SIGNPATH_API_TOKEN`          | the CI user's API token            |

The next release is then signed. Its run's summary names the certificate each file was signed
with, and the run fails rather than publish anything that did not come back signed.

## Artifact configurations

What the workflow uploads is a zip with the files at its top level.

`programs`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <zip-file>
    <pe-file path="vyro.exe">
      <authenticode-sign />
    </pe-file>
    <pe-file path="vyro-service.exe">
      <authenticode-sign />
    </pe-file>
  </zip-file>
</artifact-configuration>
```

`installer`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <zip-file>
    <msi-file path="Vyro_*_x64_en-US.msi">
      <authenticode-sign />
    </msi-file>
  </zip-file>
</artifact-configuration>
```

The installer also carries `PawnIO_setup.exe` (signed by its author) and `PresentMon.exe` (signed
by Intel) as they come: they are not signed again.

## Checking a signed release

On Windows, right-click the MSI → Properties → Digital Signatures: the signer is SignPath
Foundation's certificate for Vyro. Or in PowerShell:

```powershell
Get-AuthenticodeSignature .\Vyro_2.0.0_x64_en-US.msi | Format-List Status, SignerCertificate
```

`Status` is `Valid`. The same for `C:\Program Files\Vyro\vyro.exe` once installed.
