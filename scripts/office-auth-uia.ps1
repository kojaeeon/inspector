# Windows PowerShell 5.1; requires a normal interactive desktop.
# Only fills a foreground Windows Security dialog when its UIA text contains
# the exact configured authentication host. Never submits the dialog.
param(
    [string]$TargetHost = 'scadmin.standardchartered.co.kr',
    [int]$PollMs = 750
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ForegroundWindow {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
'@
$u = [Environment]::GetEnvironmentVariable('OFFICE_AUTH_USER')
$p = [Environment]::GetEnvironmentVariable('OFFICE_AUTH_PASSWORD')
if ([string]::IsNullOrWhiteSpace($u) -or [string]::IsNullOrEmpty($p)) {
    throw 'Set OFFICE_AUTH_USER and OFFICE_AUTH_PASSWORD in this session first.'
}
if ($TargetHost -notmatch '^[a-zA-Z0-9.-]+$') { throw 'Invalid host' }
$office = @('EXCEL', 'WINWORD', 'POWERPNT')
$processed = @{}
$lastHwnd = [IntPtr]::Zero
Write-Host 'Watching Office and Windows Security prompts. Ctrl+C to stop.'
while ($true) {
    try {
        $running = @(Get-Process -Name $office -ErrorAction SilentlyContinue).Count -gt 0
        if (-not $running) {
            $processed.Clear()
            Start-Sleep -Milliseconds $PollMs
            continue
        }
        $hwnd = [ForegroundWindow]::GetForegroundWindow()
        if ($hwnd -eq [IntPtr]::Zero) {
            Start-Sleep -Milliseconds $PollMs
            continue
        }
        if ($hwnd -ne $lastHwnd) {
            $processed.Clear()
            $lastHwnd = $hwnd
        }
        $id = $hwnd.ToInt64().ToString()
        if ($processed.ContainsKey($id)) {
            Start-Sleep -Milliseconds $PollMs
            continue
        }
        $root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
        $title = $root.Current.Name
        if ($title -notmatch 'Windows Security|Windows 보안') {
            Start-Sleep -Milliseconds $PollMs
            continue
        }
        $nodes = $root.FindAll(
            [System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.Condition]::TrueCondition
        )
        $names = New-Object System.Collections.Generic.List[string]
        $edits = New-Object System.Collections.Generic.List[System.Windows.Automation.AutomationElement]
        for ($i = 0; $i -lt $nodes.Count; $i++) {
            $node = $nodes.Item($i)
            try {
                $type = $node.Current.ControlType
                $name = $node.Current.Name
                if ($type -eq [System.Windows.Automation.ControlType]::Text -or
                    $type -eq [System.Windows.Automation.ControlType]::Hyperlink) {
                    if ($name) { $names.Add($name) }
                }
                if ($type -eq [System.Windows.Automation.ControlType]::Edit) {
                    $edits.Add($node)
                }
            } catch { continue }
        }
        # Exact hostname token, not a substring of a longer hostname.
        $escaped = [regex]::Escape($TargetHost)
        $pattern = '(?i)(?<![a-z0-9.-])' + $escaped + '(?![a-z0-9.-])'
        $matched = $false
        foreach ($line in $names) {
            if ($line -match $pattern) { $matched = $true; break }
        }
        if (-not $matched) {
            Start-Sleep -Milliseconds $PollMs
            continue
        }
        $userEdit = $null
        $passEdit = $null
        foreach ($edit in $edits) {
            try {
                if ($edit.Current.IsPassword) { $passEdit = $edit }
                elseif ($null -eq $userEdit) { $userEdit = $edit }
            } catch {}
        }
        if ($null -eq $userEdit -or $null -eq $passEdit) {
            Write-Host 'Matching host found, but both editable fields were not identified.'
            $processed[$id] = $true
            continue
        }
        # ValuePattern is used instead of blind keyboard input. If unsupported,
        # fail closed; never type a password into an unknown focused control.
        $userPattern = $null
        $passPattern = $null
        $valueId = [System.Windows.Automation.ValuePattern]::Pattern
        if (-not $userEdit.TryGetCurrentPattern($valueId, [ref]$userPattern) -or
            -not $passEdit.TryGetCurrentPattern($valueId, [ref]$passPattern)) {
            Write-Host 'Matching host found; UIA ValuePattern unavailable. No input sent.'
            $processed[$id] = $true
            continue
        }
        if ($userPattern.Current.IsReadOnly -or $passPattern.Current.IsReadOnly) {
            $processed[$id] = $true
            continue
        }
        $userPattern.SetValue($u)
        $passPattern.SetValue($p)
        $processed[$id] = $true
        Write-Host 'Fields filled. Choose Confirm or Cancel manually.'
    } catch {
        # Do not print exception details: UIA errors may include sensitive data.
        Write-Warning 'Credential window inspection/input failed; no automatic submission.'
    }
    Start-Sleep -Milliseconds $PollMs
}
