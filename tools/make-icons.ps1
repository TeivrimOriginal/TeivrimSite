# Generates the PWA / Android launcher icons from the same design as
# frontend/static/icon.svg.
#
#   powershell -File tools\make-icons.ps1
param(
    [string]$OutDir = "frontend\static",
    # Legacy launcher icons for API 24-25, which predate <adaptive-icon>. Without
    # these the app has no icon at all on those versions.
    [string]$AndroidDir = "android\app\src\main\res"
)

Add-Type -AssemblyName System.Drawing

function New-Icon {
    param(
        [int]$Size,
        [string]$OutFile,
        [bool]$Maskable,
        [bool]$Round = $false
    )

    $bmp = New-Object System.Drawing.Bitmap($Size, $Size)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.Clear([System.Drawing.Color]::Transparent)

    # A maskable icon is cropped to a circle by the launcher, so the artwork
    # is inset and the corner radius is halved. The legacy round variant is a
    # circle the launcher does not crop, so it fills the whole square.
    $pad = if ($Maskable) { [int]($Size * 0.16) } else { 0 }
    $radius = if ($Maskable) { [int]($Size * 0.28) } else { [int]($Size * 0.22) }

    $rect = New-Object System.Drawing.RectangleF(
        [float]$pad, [float]$pad, [float]($Size - 2 * $pad), [float]($Size - 2 * $pad))

    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF($rect.X, $rect.Y)),
        (New-Object System.Drawing.PointF($rect.Right, $rect.Bottom)),
        [System.Drawing.Color]::FromArgb(255, 71, 87),
        [System.Drawing.Color]::FromArgb(255, 138, 61))

    $shape = New-Object System.Drawing.Drawing2D.GraphicsPath
    if ($Round) {
        $shape.AddEllipse($rect)
    } else {
        $d = $radius * 2
        $shape.AddArc($rect.X, $rect.Y, $d, $d, 180, 90)
        $shape.AddArc($rect.Right - $d, $rect.Y, $d, $d, 270, 90)
        $shape.AddArc($rect.Right - $d, $rect.Bottom - $d, $d, $d, 0, 90)
        $shape.AddArc($rect.X, $rect.Bottom - $d, $d, $d, 90, 90)
        $shape.CloseFigure()
    }
    $g.FillPath($brush, $shape)

    # Three strokes, matching the SVG mark: crossbar, stem, lower arc.
    $inset = if ($Maskable) { $Size * 0.10 } else { 0 }
    $cx = [float]$Size * 0.5
    $top = [float]$Size * 0.31 + $inset
    $bottom = [float]$Size * 0.70 + $inset
    $half = [float]$Size * 0.20

    $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::White, [float]($Size * 0.05))
    $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round

    $g.DrawLine($pen, ($cx - $half), $top, ($cx + $half), $top)
    $g.DrawLine($pen, $cx, $top, $cx, $bottom)

    $arcWidth = $half * 1.7
    $arcHeight = [float]$Size * 0.26
    $arcRect = New-Object System.Drawing.RectangleF(
        ($cx - $arcWidth / 2), ($bottom - ($arcHeight / 2)), $arcWidth, $arcHeight)
    $g.DrawArc($pen, $arcRect, 0, 180)

    $dir = Split-Path -Parent $OutFile
    if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }

    $bmp.Save($OutFile, [System.Drawing.Imaging.ImageFormat]::Png)

    $g.Dispose()
    $bmp.Dispose()
    $pen.Dispose()
    $brush.Dispose()
    $shape.Dispose()

    Write-Host ("{0} ({1}x{1}) {2} bytes" -f $OutFile, $Size, (Get-Item $OutFile).Length)
}

New-Icon -Size 192  -OutFile (Join-Path $OutDir "icon-192.png")         -Maskable $false
New-Icon -Size 512  -OutFile (Join-Path $OutDir "icon-512.png")         -Maskable $false
New-Icon -Size 1024 -OutFile (Join-Path $OutDir "icon-1024.png")        -Maskable $false
New-Icon -Size 512  -OutFile (Join-Path $OutDir "icon-maskable-512.png") -Maskable $true

# Legacy launcher icons. minSdk is 24 and <adaptive-icon> arrived in 26, so these
# are the icons on API 24 and 25 rather than a belt-and-braces extra.
$densities = [ordered]@{
    "mdpi" = 48
    "hdpi" = 72
    "xhdpi" = 96
    "xxhdpi" = 144
    "xxxhdpi" = 192
}
foreach ($entry in $densities.GetEnumerator()) {
    $dir = Join-Path $AndroidDir "mipmap-$($entry.Key)"
    New-Icon -Size $entry.Value -OutFile (Join-Path $dir "ic_launcher.png")      -Maskable $false
    New-Icon -Size $entry.Value -OutFile (Join-Path $dir "ic_launcher_round.png") -Maskable $false -Round $true
}
