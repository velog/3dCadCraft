# Every readable property of named styles of a drawing template, with their columns (tables).
# Usage: namedstyles.ps1 OUT.txt TEMPLATE.idw "Parts List (ISO)" "Hole Table (ISO)" ...
$ErrorActionPreference = 'Stop'
$out = $args[0]
$tpl = $args[1]
$wanted = $args[2..($args.Count - 1)]
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
    if ($n -in @('Application', 'Parent', 'AttributeSets', '_Parent')) { continue }
    try {
      $v = $o.$n
      if ($v -is [System.__ComObject]) {
        $c = $null; try { $c = $v.Count } catch {}
        if ($null -ne $c) {
          $items = @()
          foreach ($x in $v) {
            $t = $null; foreach ($p in 'Title', 'Name', 'DisplayName') { try { $t = $x.$p; if ($t) { break } } catch {} }
            $w = $null; try { $w = $x.Width } catch {}
            $items += ("" + $t + $(if ($w) { " w=" + $w } else { "" }))
          }
          $v = "<collection ${c}: " + ($items -join ' | ') + ">"
        } else {
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
  foreach ($w in $wanted) {
    $found = $false
    foreach ($s in $sm.Styles) { if ($s.Name -eq $w) { dump $w $s; $found = $true; break } }
    if (-not $found) { $lines.Add("===== $w : not found") }
  }
  $doc.Close($true)
} catch { $lines.Add("ERROR " + $_.Exception.Message + " line " + $_.InvocationInfo.ScriptLineNumber) }
finally { if ($started) { $app.Quit() } }
$lines | Out-File -Encoding utf8 $out
"written $out ($($lines.Count) lines)"
