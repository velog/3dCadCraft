# Dump Inventor 2027's Drawing ribbon: tabs, panels, controls (display names, command ids).
$ErrorActionPreference = 'Stop'
$out = $args[0]
try {
  $app = [Runtime.InteropServices.Marshal]::GetActiveObject('Inventor.Application')
  $started = -not $app.Visible
} catch { $app = New-Object -ComObject Inventor.Application; $started = $true }
$app.Visible = $false
$app.SilentOperation = $true
$lines = New-Object System.Collections.Generic.List[string]
function ctl($c, $ind) {
  $name = ''; $id = ''
  try { $def = $c.ControlDefinition; if ($def) { $name = $def.DisplayName; $id = $def.InternalName } } catch {}
  $t = ''; try { $t = [string]$c.ControlType } catch {}
  $lines.Add($ind + "- " + $name + " [" + $id + "] " + $t)
  try { foreach ($k in $c.ChildControls) { ctl $k ($ind + "    ") } } catch {}
}
try {
  $r = $app.UserInterfaceManager.Ribbons.Item('Drawing')
  foreach ($tab in $r.RibbonTabs) {
    $lines.Add("TAB " + $tab.DisplayName + " [" + $tab.InternalName + "]")
    foreach ($p in $tab.RibbonPanels) {
      $lines.Add("  PANEL " + $p.DisplayName + " [" + $p.InternalName + "]")
      foreach ($c in $p.CommandControls) { ctl $c "    " }
      try { foreach ($c in $p.SlideoutControls) { ctl $c "    (slideout) " } } catch {}
    }
  }
} catch { $lines.Add("ERROR " + $_.Exception.Message + " line " + $_.InvocationInfo.ScriptLineNumber) }
finally { if ($started) { $app.Quit() } }
$lines | Out-File -Encoding utf8 $out
"lines: " + $lines.Count
