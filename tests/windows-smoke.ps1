param(
    [Parameter(Mandatory = $true)]
    [string] $Inspector
)
$ErrorActionPreference = 'Stop'
$Inspector = (Resolve-Path -LiteralPath $Inspector).Path
$temp = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString()))
$watch = $null
$fixture = $null
try {
    $fakeOffice = Join-Path $temp.FullName 'WINWORD.exe'
    $fixtureSource = Join-Path $PSScriptRoot 'fixtures/dialog.rs'
    & rustc $fixtureSource -o $fakeOffice
    if ($LASTEXITCODE -ne 0) { throw 'Could not compile the synthetic dialog fixture.' }
    $dll = Join-Path $temp.FullName 'inspector_fixture.dll'
    & rustc --crate-type cdylib (Join-Path $PSScriptRoot 'fixtures/module.rs') -o $dll
    if ($LASTEXITCODE -ne 0) { throw 'Could not compile the synthetic DLL fixture.' }
    $log = Join-Path $temp.FullName 'capture.jsonl'
    $screen = Join-Path $temp.FullName 'stdout.txt'
    $errors = Join-Path $temp.FullName 'stderr.txt'
    $watch = Start-Process -FilePath $Inspector -ArgumentList @('watch', '--dll', '--duration', '8', '--out', ('"' + $log + '"')) -PassThru -RedirectStandardOutput $screen -RedirectStandardError $errors
    $readyBy = [DateTime]::UtcNow.AddSeconds(4)
    while (!(Test-Path -LiteralPath $log) -and !$watch.HasExited -and [DateTime]::UtcNow -lt $readyBy) {
        Start-Sleep -Milliseconds 30
    }
    if (!(Test-Path -LiteralPath $log)) { throw "Capture did not start. $(Get-Content -LiteralPath $errors -Raw)" }
    $fixture = Start-Process -FilePath $fakeOffice -ArgumentList ('"' + $dll + '"') -PassThru
    if (!$fixture.WaitForExit(6500) -or $fixture.ExitCode -ne 0) { throw 'The synthetic dialog fixture failed.' }
    if (!$watch.WaitForExit(10000) -or $watch.ExitCode -ne 0) { throw "Capture failed. $(Get-Content -LiteralPath $errors -Raw)" }
    Get-Content -LiteralPath $screen
    $events = @(Get-Content -LiteralPath $log | ForEach-Object { ConvertFrom-Json -InputObject $_ } | Where-Object { $_.pid -eq $fixture.Id })
    if (@($events | Where-Object kind -eq dialog_opened).Count -ne 2) { throw 'Expected exactly two dialog openings.' }
    if (@($events | Where-Object kind -eq dialog_closed).Count -ne 2) { throw 'Expected exactly two dialog closings.' }
    foreach ($kind in @('module_baseline', 'module_added', 'module_removed')) {
        $matching = @($events | Where-Object { $_.kind -eq $kind -and ($kind -eq 'module_baseline' -or $_.module_path -like '*inspector_fixture.dll') })
        if ($matching.Count -eq 0) { throw "Missing expected DLL event: $kind" }
    }
    $summary = (& $Inspector summarize $log) -join "`n"
    if ($LASTEXITCODE -ne 0 -or $summary -notmatch 'openings=2 reopenings=1 last_reopen_gap_ms=\d+') {
        throw "Expected exactly one reopening. $summary"
    }
} finally {
    foreach ($process in @($fixture, $watch)) {
        if ($null -ne $process -and !$process.HasExited) { $process.Kill(); $process.WaitForExit() }
    }
    Remove-Item -LiteralPath $temp.FullName -Recurse -Force
}
