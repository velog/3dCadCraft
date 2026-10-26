# Reference drawing from Inventor 2027 (COM API): the 80 x 50 x 12 plate with two
# counterbored Ø6.6 holes and a Ø16 hole (as examples/drawing_demo.rs), an ISO drawing with
# a front view at 1.5:1 (hidden lines), the projected top view, a length and a diameter
# dimension and a centre mark; writes the dimension texts and a PDF.
$ErrorActionPreference = 'Stop'
$dir = $args[0]
$metric = 'C:\Users\Public\Documents\Autodesk\Inventor 2027\Templates\en-US\Metric'
try {
  $app = [Runtime.InteropServices.Marshal]::GetActiveObject('Inventor.Application')
  $started = -not $app.Visible
} catch { $app = New-Object -ComObject Inventor.Application; $started = $true }
$app.Visible = $false
$app.SilentOperation = $true
$tg = $app.TransientGeometry
$report = New-Object System.Collections.Generic.List[string]
try {
  # ---- part (cm) ----
  $part = $app.Documents.Add(12290, "$metric\Standard (mm).ipt", $true)
  $cd = $part.ComponentDefinition
  $sk = $cd.Sketches.Add($cd.WorkPlanes.Item(3))
  [void]$sk.SketchLines.AddAsTwoPointRectangle($tg.CreatePoint2d(0, 0), $tg.CreatePoint2d(8, 5))
  $prof = $sk.Profiles.AddForSolid()
  $ext = $cd.Features.ExtrudeFeatures.AddByDistanceExtent($prof, 1.2, 20993, 20481)
  # The top face: the extrusion's end face.
  $top = $ext.EndFaces.Item(1)
  # Holes as cuts: circles on the top face, through all (Ø6.6, Ø16) and 6.4 deep (Ø11).
  function cut($circles, $through, $depth) {
    $s = $cd.Sketches.Add($top)
    foreach ($c in $circles) {
      $q = $s.ModelToSketchSpace($tg.CreatePoint($c[0], $c[1], 1.2))
      [void]$s.SketchCircles.AddByCenterRadius($q, $c[2])
    }
    $pr = $s.Profiles.AddForSolid()
    $ef = $cd.Features.ExtrudeFeatures
    if ($through) {
      try { [void]$ef.AddByThroughAllExtent($pr, 20994, 20482) } catch { [void]$ef.AddByThroughAllExtent($pr, 20993, 20482) }
    } else {
      try { [void]$ef.AddByDistanceExtent($pr, $depth, 20994, 20482) } catch { [void]$ef.AddByDistanceExtent($pr, $depth, 20993, 20482) }
    }
  }
  cut @(@(1.5, 2.5, 0.33), @(6.5, 2.5, 0.33), @(4.0, 2.5, 0.8)) $true 0
  cut @(@(1.5, 2.5, 0.55), @(6.5, 2.5, 0.55)) $false 0.64
  $partPath = Join-Path $dir 'plate_ref.ipt'
  $part.SaveAs($partPath, $false)
  $report.Add("part volume cm3 = " + $cd.MassProperties.Volume)

  # ---- drawing ----
  $dwg = $app.Documents.Add(12292, "$metric\ISO.idw", $true)
  $sheet = $dwg.Sheets.Item(1)
  $report.Add("sheet = " + $sheet.Size + " " + $sheet.Width + " x " + $sheet.Height + " cm")
  # kFrontViewOrientation 10764, kHiddenLineDrawingViewStyle 32257, kFromBaseDrawingViewStyle 32260
  $base = $sheet.DrawingViews.AddBaseView($part, $tg.CreatePoint2d(11, 21.5), 1.5, 10764, 32257)
  $topv = $sheet.DrawingViews.AddProjectedView($base, $tg.CreatePoint2d(11, 12), 32260)
  $report.Add("base view = " + $base.Name + ", projected = " + $topv.Name)
  # Curves of the top view: the longest line and the circles.
  $long = $null; $len = 0; $circles = @()
  foreach ($c in $topv.DrawingCurves()) {
    $ct = $c.CurveType
    if ($ct -eq 5124) { $circles += $c }
    elseif ($ct -eq 5122) {
      $l = $c.StartPoint.DistanceTo($c.EndPoint)
      if ($l -gt $len) { $len = $l; $long = $c }
    }
  }
  $report.Add("top view curves: circles = " + $circles.Count + ", longest line on sheet = " + $len + " cm")
  $gd = $sheet.DrawingDimensions.GeneralDimensions
  if ($long) {
    $i1 = $sheet.CreateGeometryIntent($long)
    $d1 = $gd.AddLinear($tg.CreatePoint2d(11, 5), $i1)
    $report.Add("linear dimension text = '" + $d1.Text.Text + "' value cm = " + $d1.ModelValue + " style = " + $d1.Style.Name)
  }
  $big = $null
  foreach ($c in $circles) { if (-not $big -or $c.Radius -gt $big.Radius) { $big = $c } }
  if ($big) {
    $i2 = $sheet.CreateGeometryIntent($big)
    $d2 = $gd.AddDiameter($tg.CreatePoint2d(16, 8), $i2)
    $report.Add("diameter dimension text = '" + $d2.Text.Text + "' value cm = " + $d2.ModelValue)
    $cm = $sheet.Centermarks.Add($i2)
    $report.Add("centre mark style = " + $cm.Style.Name)
  }
  $dwgPath = Join-Path $dir 'plate_ref.idw'
  $dwg.SaveAs($dwgPath, $false)
  # PDF through the translator add-in.
  $pdfAddin = $app.ApplicationAddIns.ItemById('{0AC6FD96-2F4D-42CE-8BE0-8AEA580399E4}')
  $ctx = $app.TransientObjects.CreateTranslationContext()
  $ctx.Type = 13059  # kFileBrowseIOMechanism
  $opts = $app.TransientObjects.CreateNameValueMap()
  $med = $app.TransientObjects.CreateDataMedium()
  $med.FileName = (Join-Path $dir 'plate_ref.pdf')
  if ($pdfAddin.HasSaveCopyAsOptions($dwg, $ctx, $opts)) {
    $opts.Value('All_Color_AS_Black') = 1
    $opts.Value('Vector_Resolution') = 400
  }
  $pdfAddin.SaveCopyAs($dwg, $ctx, $opts, $med)
  $report.Add("pdf = " + $med.FileName)
  $dwg.Close($true)
  $part.Close($true)
} catch {
  $report.Add("ERROR: " + $_.Exception.Message + " at line " + $_.InvocationInfo.ScriptLineNumber)
} finally { if ($started) { $app.Quit() } }
$report | Out-File -Encoding utf8 (Join-Path $dir 'refdrawing.txt')
$report
