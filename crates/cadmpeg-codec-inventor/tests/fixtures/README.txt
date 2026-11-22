inventor_parameter_expressions.ipt (+ .step): a purpose-made test part for reading Inventor parameters and the
expressions that reference other parameters. Released under CC0 1.0 (public domain dedication,
https://creativecommons.org/publicdomain/zero/1.0/). No attribution needed.

Made from nothing but the recipe below, in Inventor Professional 2016, with a neutral author name ("Fixture
Builder"). The file's identity fields and every OLE stream were cleaned of the builder machine's user, computer and
owner names (same-length placeholders, then compacted), the file was reopened to confirm the same volume and
parameters, and a second independent scan found no name, e-mail address, user path or licence string.

Content: one part, one block from one sketch (rectangle 40 x 25 mm with two through holes), extruded 30 mm.
  User parameters:   Width  = 40 mm
                     Height = Width / 2           (stored by Inventor as "Width / 2 ul")
                     Hole   = 6 mm
  Model parameters (sketch and feature dimensions, driven by expressions):
                     d0 = Width                   rectangle length
                     d1 = Width / 2 + 5 mm        rectangle height
                     d2 = Height                  hole 1 centre, distance from the left edge
                     d3 = Hole                    hole 1 diameter
                     d4 = d3 * 2                  hole 2 diameter (a model parameter referencing another)
                     d5 = Height * 1.5            extrude distance
                     d6 = 0 deg                   extrude taper angle (added by Inventor)
Volume 25758.85 mm3 (40 x 25 x 30 minus the two holes), one body, 8 faces.

Please use a licensed copy.
