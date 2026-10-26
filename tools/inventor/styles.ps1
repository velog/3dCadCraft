# Reads the ISO drawing template's styles from Inventor 2027 through its COM API.
$ErrorActionPreference = 'Stop'
$out = $args[0]
$tpls = $args[1..($args.Length-1)]
try {
  $app = [Runtime.InteropServices.Marshal]::GetActiveObject('Inventor.Application')
  # A hidden instance (no window) is one a script left behind: ours to close.
  $started = -not $app.Visible
} catch { $app = New-Object -ComObject Inventor.Application; $started = $true }
$app.Visible = $false
$app.SilentOperation = $true
$all = [ordered]@{}
try {
foreach ($tpl in $tpls) {
$doc = $app.Documents.Add(12292, $tpl, $false)
$sm = $doc.StylesManager
$r = [ordered]@{ template = $tpl }
$std = $sm.ActiveStandardStyle
$r.standard = [ordered]@{ name = $std.Name; firstAngle = $std.FirstAngleProjection }
function props($o, $names) {
  $h = [ordered]@{}
  foreach ($n in $names) { try { $h[$n] = $o.$n } catch { $h[$n] = 'ERR' } }
  $h
}
$r.dimension = @()
foreach ($s in $sm.DimensionStyles) {
  $r.dimension += props $s @('Name','LinearPrecision','AngularPrecision','DecimalMarker','TrailingZeroDisplay','LeadingZeroDisplay','ArrowheadType','ArrowheadSize','ArrowheadHeight','ExtensionLineExtension','ExtensionLineGap','GapToText','ParallelDimensionSpacing','PartOffset','LinearDimensionTextOrientation','ShowDiameterSymbolForCircle','LinearPrefix','LinearSuffix')
}
$r.text = @()
foreach ($s in $sm.TextStyles) { $r.text += props $s @('Name','Font','FontSize','WidthScale','LineSpacing','Bold','Italic') }
$r.leader = @()
foreach ($s in $sm.LeaderStyles) { $r.leader += props $s @('Name','ArrowheadType','ArrowheadSize','ArrowheadHeight','LineWeight') }
$r.centermark = @()
foreach ($s in $sm.CentermarkStyles) { $r.centermark += props $s @('Name','MarkSize','GapDistance','ExtensionLength','Overshoot','LineWeight') }
$r.hatch = @()
foreach ($s in $sm.HatchStyles) { $r.hatch += props $s @('Name','Angle','Scale','Shift','DoublePattern','LineWeight') }
$r.layers = @()
foreach ($s in $sm.Layers) { $r.layers += props $s @('Name','LineType','LineWeight','Visible','Plot') }
$r.objectDefaults = @()
try {
  $od = $std.ActiveObjectDefaults
  $r.objectDefaultsName = $od.Name
} catch {}
$doc.Close($true)
$all[[IO.Path]::GetFileNameWithoutExtension($tpl)] = $r
}
} finally { if ($started) { $app.Quit() } }
$all | ConvertTo-Json -Depth 6 | Out-File -Encoding utf8 $out
"written $out"
