# Runs every single-file example through the interpreter and through the
# native compiler, and checks that both agree on stdout and exit code.
# Runtime errors go to stderr in both backends, so they are not compared.
$dirs = 'examples/native', 'examples/scoping', 'examples/operators', 'examples/iteration', 'examples/stdlib', 'examples/files'
$failures = 0

foreach ($dir in $dirs) {
    foreach ($file in Get-ChildItem $dir -Filter *.kairo) {
        $exe = [System.IO.Path]::ChangeExtension($file.FullName, 'exe')
        Remove-Item $exe -ErrorAction SilentlyContinue

        $expected = @(cargo run -q -p kairo -- run $file.FullName 2>$null)
        $expectedCode = $LASTEXITCODE

        cargo run -q -p kairo -- build $file.FullName 2>$null | Out-Null
        if (-not (Test-Path $exe)) {
            Write-Host "FAIL  $($file.Name): native build failed"
            $failures++
            continue
        }
        $actual = @(& $exe 2>$null)
        $actualCode = $LASTEXITCODE

        if ($actualCode -eq $expectedCode -and ($actual -join "`n") -eq ($expected -join "`n")) {
            Write-Host "ok    $($file.Name)"
        } else {
            Write-Host "FAIL  $($file.Name): interpreter exit $expectedCode, native exit $actualCode"
            $failures++
        }
    }
}
exit ([int]($failures -gt 0))