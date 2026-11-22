# Dot-run by runner.ps1: $app, $Work, Log() are in scope.
$tg = $app.TransientGeometry
$tpl = 'C:\Users\Public\Documents\Autodesk\Inventor 2016\Templates\Metric\Standard (mm).ipt'
$doc = $app.Documents.Add(12290, $tpl, $true)
$step = 'start'
try {
  Log ("units " + $doc.UnitsOfMeasure.GetStringFromType($doc.UnitsOfMeasure.LengthUnits))
  $cd = $doc.ComponentDefinition
  $ups = $cd.Parameters.UserParameters
  $step = 'user params'
  [void]$ups.AddByExpression('Width', '40 mm', 'mm')
  [void]$ups.AddByExpression('Height', 'Width / 2', 'mm')
  [void]$ups.AddByExpression('Hole', '6 mm', 'mm')
  $step = 'sketch'
  $sk = $cd.Sketches.Add($cd.WorkPlanes.Item(3))
  $l = $sk.SketchLines.AddAsTwoPointRectangle($tg.CreatePoint2d(0, 0), $tg.CreatePoint2d(4, 2.5))
  $c1 = $sk.SketchCircles.AddByCenterRadius($tg.CreatePoint2d(2, 1.25), 0.3)
  $c2 = $sk.SketchCircles.AddByCenterRadius($tg.CreatePoint2d(3.2, 1.25), 0.6)
  $dc = $sk.DimensionConstraints
  $step = 'dims'
  $dX = $dc.AddTwoPointDistance($l.Item(1).StartSketchPoint, $l.Item(1).EndSketchPoint, 19201, $tg.CreatePoint2d(2, -0.5))
  $dY = $dc.AddTwoPointDistance($l.Item(2).StartSketchPoint, $l.Item(2).EndSketchPoint, 19202, $tg.CreatePoint2d(4.5, 1.25))
  $dC = $dc.AddTwoPointDistance($l.Item(1).StartSketchPoint, $c1.CenterSketchPoint, 19201, $tg.CreatePoint2d(1, 0.6))
  $dD1 = $sk.DimensionConstraints.AddDiameter($c1, $tg.CreatePoint2d(2, 0.4))
  $dD2 = $sk.DimensionConstraints.AddDiameter($c2, $tg.CreatePoint2d(3.2, 2.2))
  try { [void]$sk.GeometricConstraints.AddGround($l.Item(1).StartSketchPoint) } catch { Log ("ground: " + $_.Exception.Message) }
  $step = 'expressions'
  $dX.Parameter.Expression = 'Width'
  $dY.Parameter.Expression = 'Width / 2 + 5 mm'
  $dC.Parameter.Expression = 'Height'
  $dD1.Parameter.Expression = 'Hole'
  $dD2.Parameter.Expression = ($dD1.Parameter.Name + ' * 2')
  Log ("dims: " + $dX.Parameter.Name + "=Width " + $dY.Parameter.Name + "=Width/2+5mm " + $dC.Parameter.Name + "=Height " + $dD1.Parameter.Name + "=Hole " + $dD2.Parameter.Name + "=" + $dD1.Parameter.Name + "*2")
  $step = 'extrude'
  $prof = $sk.Profiles.AddForSolid()
  $ef = $cd.Features.ExtrudeFeatures
  $def = $ef.CreateExtrudeDefinition($prof, 20481)
  $def.SetDistanceExtent('Height * 1.5', 20993)
  $feat = $ef.Add($def)
  $feat.Name = 'Block'
  $doc.Update()
  $step = 'properties'
  $names = '|Author|Company|Manager|Last Saved By|Designer|Engineer|Checked By|Creator|'
  foreach ($ps in $doc.PropertySets) { foreach ($p in $ps) { if ($names.Contains('|' + $p.Name + '|')) { try { $p.Value = 'Fixture Builder' } catch { Log ("prop " + $p.Name + ": " + $_.Exception.Message) } } } }
  $step = 'save'
  $ipt = Join-Path $Work 'inventor_parameter_expressions.ipt'
  $doc.SaveAs($ipt, $false)
  $step = 'step'
  $tr = $app.ApplicationAddIns.ItemById('{90AF7F40-0C01-11D5-8E83-0010B541CD80}')
  $ctx = $app.TransientObjects.CreateTranslationContext(); $ctx.Type = 13059
  $opt = $app.TransientObjects.CreateNameValueMap()
  if ($tr.HasSaveCopyAsOptions($doc, $ctx, $opt)) { $opt.Value('ApplicationProtocolType') = 3 }
  $med = $app.TransientObjects.CreateDataMedium(); $med.FileName = (Join-Path $Work 'inventor_parameter_expressions.step')
  $tr.SaveCopyAs($doc, $ctx, $opt, $med)
  Log 'saved'
} catch { Log ("FAILED at " + $step + ": " + $_.Exception.Message) }
finally { try { $doc.Close($true) } catch { } }
