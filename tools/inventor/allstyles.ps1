# Lists every style collection of an Inventor drawing template and every readable property of the
# style the template's ISO standard uses for each (or the first one): parts list, balloon, hole
# table, revision table, table, surface texture, weld symbol, feature control frame, datum, ID styles.
# Usage: allstyles.ps1 OUT.txt TEMPLATE.idw
$ErrorActionPreference = 'Stop'
$out = $args[0]
$tpl = $args[1]
try {
  $app = [Runtime.InteropServices.Marshal]::GetActiveObject('Inventor.Application')
  $started = -not $app.Visible
} catch { $app = New-Object -ComObject Inventor.Application; $started = $true }
$app.Visible = $false
$app.SilentOperation = $true
$lines = New-Object System.Collections.Generic.List[string]
function dump($title, $o) {
  $lines.Add("===== $title")
  foreach ($m in ($o | Get-Member -MemberType Property)) {
    $n = $m.Name
    if ($n -in @('Application', 'Parent', 'AttributeSets')) { continue }
    try {
      $v = $o.$n
      if ($v -is [System.__ComObject]) {
        $c = $null
        try { $c = $v.Count } catch {}
        if ($null -ne $c) { $v = "<collection $c>" } else {
          $nm = $null; try { $nm = $v.Name } catch {}
          $v = if ($nm) { "<object $nm>" } else { '<object>' }
        }
      }
      $lines.Add("$n = $v")
    } catch { $lines.Add("$n = <error>") }
  }
}
try {
  $doc = $app.Documents.Add(12292, $tpl, $false)
  $sm = $doc.StylesManager
  foreach ($m in ($sm | Get-Member -MemberType Property)) {
    $n = $m.Name
    if (-not $n.EndsWith('Styles')) { continue }
    try {
      $col = $sm.$n
      $names = @(); foreach ($s in $col) { $names += $s.Name }
      $lines.Add("##### $n : " + ($names -join ' | '))
      $pick = $null
      foreach ($s in $col) { if ($s.Name -like '*ISO*') { $pick = $s; break } }
      if ($null -eq $pick -and $col.Count -gt 0) { $pick = $col.Item(1) }
      if ($null -ne $pick) { dump ($n + ': ' + $pick.Name) $pick }
    } catch { $lines.Add("##### $n : <error " + $_.Exception.Message + ">") }
  }
  # Parts list columns of the ISO parts list style.
  try {
    foreach ($s in $sm.PartsListStyles) {
      if ($s.Name -like '*ISO*') {
        $k = 1
        foreach ($c in $s.Columns) {
          $lines.Add("partslist column $k : " + $c.Title + " width " + $c.Width + " prop " + $c.PropertyType)
          $k++
        }
        break
      }
    }
  } catch { $lines.Add("partslist columns <error " + $_.Exception.Message + ">") }
  $doc.Close($true)
} catch { $lines.Add("ERROR " + $_.Exception.Message + " line " + $_.InvocationInfo.ScriptLineNumber) }
finally { if ($started) { $app.Quit() } }
$lines | Out-File -Encoding utf8 $out
"written $out ($($lines.Count) lines)"
