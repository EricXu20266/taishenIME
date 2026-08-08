$dest = "$env:LOCALAPPDATA\TaishenIME"
$procs = Get-Process | Where-Object { $_.Path -like "*TaishenIME*" }
if ($procs) {
    $procs | Select-Object Name, Id, Path | Format-Table
} else {
    Write-Output "No process using TaishenIME dir"
}
try {
    Remove-Item "$dest\taishen_ime_v015.dll" -Force -ErrorAction Stop
    Write-Output "v015 deleted"
} catch {
    Write-Output ("LOCKED: " + $_.Exception.Message.Split("`n")[0])
}
Get-ChildItem $dest | Select-Object Name, Length
