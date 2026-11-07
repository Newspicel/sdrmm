param(
    [Parameter(Mandatory)][ValidateSet('nsis', 'msi')][string]$Kind,
    [Parameter(Mandatory)][string]$Old,
    [Parameter(Mandatory)][string]$New,
    [Parameter(Mandatory)][string]$Version
)

$ErrorActionPreference = 'Stop'
$failures = [System.Collections.Generic.List[string]]::new()

function Install-Old {
    if ($Kind -eq 'nsis') {
        Invoke-Installer (Get-Item "$Old/*-setup.exe").FullName @('/S')
    } else {
        Invoke-Installer 'msiexec.exe' @('/i', (Get-Item "$Old/*.msi").FullName, '/qn')
    }
}

function Update-AsTauri {
    if ($Kind -eq 'nsis') {
        Invoke-Installer (Get-Item "$New/*-setup.exe").FullName @('/P', '/UPDATE')
    } else {
        Invoke-Installer 'msiexec.exe' @('/i', (Get-Item "$New/*.msi").FullName, '/passive', '/promptrestart')
    }
}

function Invoke-Installer([string]$File, [string[]]$Arguments) {
    Write-Host "> $File $($Arguments -join ' ')"
    $process = Start-Process $File -ArgumentList $Arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "$File exited with $($process.ExitCode)"
    }
}

function Get-Entries {
    $keys = @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
    )
    Get-ItemProperty $keys -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -like 'SDRmm*' } |
        Select-Object DisplayName, DisplayVersion, InstallLocation, PSChildName
}

function Get-Shortcuts {
    $shell = New-Object -ComObject WScript.Shell
    $roots = @(
        [Environment]::GetFolderPath('Programs'),
        [Environment]::GetFolderPath('CommonPrograms'),
        [Environment]::GetFolderPath('Desktop'),
        [Environment]::GetFolderPath('CommonDesktopDirectory')
    )
    foreach ($root in $roots) {
        Get-ChildItem $root -Recurse -Filter 'SDRmm*.lnk' -ErrorAction SilentlyContinue | ForEach-Object {
            [pscustomobject]@{ Shortcut = $_.FullName; Target = $shell.CreateShortcut($_.FullName).TargetPath }
        }
    }
}

function Get-Executables {
    $dirs = @("$env:LOCALAPPDATA\SDRmm", "$env:ProgramFiles\SDRmm")
    Get-ChildItem $dirs -Filter '*.exe' -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -notlike 'uninstall*' } |
        Select-Object -ExpandProperty FullName
}

function Show-State([string]$Title) {
    Write-Host "== $Title"
    Get-Entries | Format-Table -AutoSize | Out-String -Width 300 | Write-Host
    Get-Shortcuts | Format-Table -AutoSize | Out-String -Width 300 | Write-Host
    Get-Executables | Write-Host
}

function Test-Entries {
    $entries = @(Get-Entries)
    if ($entries.Count -ne 1) {
        $failures.Add("$($entries.Count) uninstall entries for SDRmm, expected 1")
    }
    $stale = @($entries | Where-Object { $_.DisplayVersion -ne $Version })
    if ($stale.Count -gt 0) {
        $failures.Add("uninstall entries still on an old version: $($stale.DisplayVersion -join ', ')")
    }
}

function Test-Shortcuts {
    $shortcuts = @(Get-Shortcuts)
    if ($shortcuts.Count -eq 0) {
        $failures.Add('no SDRmm shortcut left')
    }
    foreach ($shortcut in $shortcuts) {
        if ((Split-Path $shortcut.Target -Leaf) -ne 'SDRmm.exe' -or -not (Test-Path $shortcut.Target)) {
            $failures.Add("$($shortcut.Shortcut) opens $($shortcut.Target), not the updated SDRmm.exe")
        }
    }
}

function Test-Executables {
    $old = @(Get-Executables | Where-Object { (Split-Path $_ -Leaf) -eq 'sdrmm-desktop.exe' })
    foreach ($exe in $old) {
        $failures.Add("the Tauri executable is still installed: $exe")
    }
    if (-not (Get-Executables | Where-Object { (Split-Path $_ -Leaf) -eq 'SDRmm.exe' })) {
        $failures.Add('SDRmm.exe is not installed')
    }
}

Install-Old
Show-State 'after installing the Tauri release'
Update-AsTauri
Show-State 'after the update'
Test-Entries
Test-Shortcuts
Test-Executables

if ($failures.Count -gt 0) {
    $failures | ForEach-Object { Write-Host "::error::$Kind update: $_" }
    exit 1
}
Write-Host "$Kind update from Tauri migrates cleanly"
