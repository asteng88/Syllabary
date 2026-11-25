# Registers per-user (HKCU) Windows file associations and custom Syllabary document icons
# (no administrator privileges required). Pass -Extensions @("wpx","docx","pdf","gdoc","md","rtf","txt")
# to choose which extensions to associate with Syllabary.
param(
    [string]$ExePath = (Resolve-Path "$PSScriptRoot\..\..\target\release\syllabary.exe").Path,
    [string[]]$Extensions = @("wpx", "docx", "pdf", "gdoc", "md", "rtf", "txt")
)

$map = @{
    "wpx"  = @{ ProgId = "Syllabary.Document.wpx";  Desc = "Syllabary Document";       IconIdx = 1 }
    "docx" = @{ ProgId = "Syllabary.Document.docx"; Desc = "Word Document (Syllabary)"; IconIdx = 2 }
    "pdf"  = @{ ProgId = "Syllabary.Document.pdf";  Desc = "PDF Document (Syllabary)";  IconIdx = 3 }
    "gdoc" = @{ ProgId = "Syllabary.Document.gdoc"; Desc = "Google Doc (Syllabary)";    IconIdx = 4 }
    "md"   = @{ ProgId = "Syllabary.Document.md";   Desc = "Markdown Document (Syllabary)"; IconIdx = 5 }
    "rtf"  = @{ ProgId = "Syllabary.Document.rtf";  Desc = "Rich Text Document (Syllabary)"; IconIdx = 6 }
    "txt"  = @{ ProgId = "Syllabary.Document.txt";  Desc = "Text Document (Syllabary)"; IconIdx = 7 }
}

foreach ($ext in $Extensions) {
    $k = $ext.TrimStart('.').ToLowerInvariant()
    if (-not $map.ContainsKey($k)) { continue }
    $info = $map[$k]
    $progId = $info.ProgId
    $base = "HKCU:\Software\Classes\$progId"
    New-Item -Path "$base\DefaultIcon" -Force | Out-Null
    New-Item -Path "$base\shell\open\command" -Force | Out-Null
    Set-ItemProperty -Path $base -Name "(Default)" -Value $info.Desc
    Set-ItemProperty -Path "$base\DefaultIcon" -Name "(Default)" -Value "`"$ExePath`",$($info.IconIdx)"
    Set-ItemProperty -Path "$base\shell\open\command" -Name "(Default)" -Value "`"$ExePath`" `"%1`""

    $extKey = "HKCU:\Software\Classes\.$k"
    New-Item -Path "$extKey\OpenWithProgids" -Force | Out-Null
    Set-ItemProperty -Path "$extKey\OpenWithProgids" -Name $progId -Value ""
    Set-ItemProperty -Path $extKey -Name "(Default)" -Value $progId
    Write-Host "Registered .$k -> $progId (icon index $($info.IconIdx))"
}

# Register Syllabary under Applications\syllabary.exe so Windows "Open with" & Default Apps list it cleanly
$appBase = "HKCU:\Software\Classes\Applications\syllabary.exe"
New-Item -Path "$appBase\DefaultIcon" -Force | Out-Null
New-Item -Path "$appBase\shell\open\command" -Force | Out-Null
New-Item -Path "$appBase\SupportedTypes" -Force | Out-Null
Set-ItemProperty -Path $appBase -Name "FriendlyAppName" -Value "Syllabary"
Set-ItemProperty -Path "$appBase\DefaultIcon" -Name "(Default)" -Value "`"$ExePath`",0"
Set-ItemProperty -Path "$appBase\shell\open\command" -Name "(Default)" -Value "`"$ExePath`" `"%1`""
foreach ($ext in $Extensions) {
    $k = $ext.TrimStart('.').ToLowerInvariant()
    Set-ItemProperty -Path "$appBase\SupportedTypes" -Name ".$k" -Value ""
}

# Notify Windows Shell to refresh file-type icons immediately
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class ShellNotify {
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(uint wEventId, uint uFlags, IntPtr dwItem1, IntPtr dwItem2);
}
"@
[ShellNotify]::SHChangeNotify(0x08000000, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero)
Write-Host "Shell icon cache refreshed."
