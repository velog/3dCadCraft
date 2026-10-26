# Lists every readable property (name = value) of one style of each type in Inventor's
# ISO template, so the reference reads the right names (COM late binding).
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
  if ($null -eq $o) { $lines.Add("(none)"); return }
  foreach ($m in ($o | Get-Member -MemberType Property)) {
    $n = $m.Name
    try {
      $v = $o.$n
      if ($v -is [System.__ComObject]) { $v = '<object>' }
      $lines.Add("$n = $v")
    } catch { $lines.Add("$n = <error>") }
  }
}
try {
  $doc = $app.Documents.Add(12292, $tpl, $false)
  $sm = $doc.StylesManager
  dump 'Standard' $sm.ActiveStandardStyle
  dump 'Dimension Default (ISO)' $sm.DimensionStyles.Item('Default (ISO)')
  dump 'Text Note Text (ISO)' $sm.TextStyles.Item('Note Text (ISO)')
  foreach ($x in $sm.LeaderStyles) { if ($x.Name -like '*ISO*') { dump ('Leader ' + $x.Name) $x; break } }
  dump 'CenterMark' $sm.CentermarkStyles.Item('Center Mark (ISO)')
  foreach ($x in $sm.HatchStyles) { if ($x.Name -like '*ISO*') { dump ('Hatch ' + $x.Name) $x; break } }
  dump 'ObjectDefaults' $sm.ActiveStandardStyle.ActiveObjectDefaults
  $doc.Close($true)
} finally { if ($started) { $app.Quit() } }
$lines | Out-File -Encoding utf8 $out
"written $out ($($lines.Count) lines)"
