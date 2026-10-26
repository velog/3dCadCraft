# The ISO template's border and title block per sheet size (Inventor 2027, COM).
$ErrorActionPreference = 'Stop'
$out = $args[0]
try {
  $app = [Runtime.InteropServices.Marshal]::GetActiveObject('Inventor.Application')
  $started = -not $app.Visible
} catch { $app = New-Object -ComObject Inventor.Application; $started = $true }
$app.Visible = $false
$app.SilentOperation = $true
$lines = New-Object System.Collections.Generic.List[string]
try {
  $dwg = $app.Documents.Add(12292, 'C:\Users\Public\Documents\Autodesk\Inventor 2027\Templates\en-US\Metric\ISO.idw', $false)
  $sheet = $dwg.Sheets.Item(1)
  foreach ($m in ($sheet.Border | Get-Member -MemberType Property)) {
    try { $v = $sheet.Border.($m.Name); if ($v -is [System.__ComObject]) { $v = '<object>' }; $lines.Add("border." + $m.Name + " = " + $v) } catch {}
  }
  $def = $sheet.Border.Definition
  foreach ($m in ($def | Get-Member -MemberType Property)) {
    try { $v = $def.($m.Name); if ($v -is [System.__ComObject]) { $v = '<object>' }; $lines.Add("borderdef." + $m.Name + " = " + $v) } catch {}
  }
  $tb = $sheet.TitleBlock
  $lines.Add("titleblock = " + $tb.Name + " pos " + $tb.Position.X + "," + $tb.Position.Y)
  $r = $tb.RangeBox
  $lines.Add("titleblock range = " + $r.MinPoint.X + "," + $r.MinPoint.Y + " .. " + $r.MaxPoint.X + "," + $r.MaxPoint.Y)
  foreach ($size in 9993..10000) {
    try {
      $sheet.Size = $size
      $b = $sheet.Border
      $lines.Add("size " + $size + ": " + $sheet.Width + " x " + $sheet.Height + " border range " + $b.RangeBox.MinPoint.X + "," + $b.RangeBox.MinPoint.Y + " .. " + $b.RangeBox.MaxPoint.X + "," + $b.RangeBox.MaxPoint.Y)
    } catch { $lines.Add("size " + $size + ": " + $_.Exception.Message) }
  }
  $dwg.Close($true)
} catch { $lines.Add("ERROR " + $_.Exception.Message + " line " + $_.InvocationInfo.ScriptLineNumber) }
finally { if ($started) { $app.Quit() } }
$lines | Out-File -Encoding utf8 $out
$lines
