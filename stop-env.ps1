[CmdletBinding()]
param()

$ErrorActionPreference = "Continue"

$Root = (Resolve-Path $PSScriptRoot).Path
$EnvRoot = Join-Path $Root "env"
$MysqlPort = 13306
$RedisPort = 16379
$RustPort = 13001
$JavaPort = 18086
$FrontendPort = 15173
$rootPattern = [regex]::Escape($Root)
$envPattern = [regex]::Escape($EnvRoot)

function Get-ProcessSnapshot {
    try {
        return @(Get-CimInstance Win32_Process -ErrorAction Stop)
    } catch {
        Write-Warning "Could not inspect process command lines. Port-owned project processes will still be checked by executable path."
        return @()
    }
}

function Get-ProcessPathSafe {
    param([int]$ProcessId)

    try {
        $process = Get-Process -Id $ProcessId -ErrorAction Stop
        return $process.Path
    } catch {
        return $null
    }
}

function Add-TargetProcess {
    param(
        [Parameter(Mandatory = $true)]
        [System.Collections.Generic.HashSet[int]]$TargetIds,
        [Parameter(Mandatory = $true)]
        [int]$ProcessId
    )

    if ($ProcessId -gt 0) {
        [void]$TargetIds.Add($ProcessId)
    }
}

function Stop-TargetProcess {
    param([int]$ProcessId)

    try {
        Stop-Process -Id $ProcessId -Force -ErrorAction Stop
        Write-Host "Stopped process $ProcessId."
    } catch {
        if ($_.Exception -and $_.Exception.Message -notmatch "Cannot find a process") {
            Write-Warning "Could not stop process $ProcessId`: $($_.Exception.Message)"
        }
    }
}

$snapshot = Get-ProcessSnapshot
$targetIds = [System.Collections.Generic.HashSet[int]]::new()
$targetNames = @(
    "cmd.exe",
    "cargo.exe",
    "rustc.exe",
    "server.exe",
    "java.exe",
    "node.exe",
    "esbuild.exe",
    "mvn.exe",
    "mysqld.exe",
    "redis-server.exe"
)

foreach ($process in $snapshot) {
    if (-not $process.CommandLine) {
        continue
    }

    if ($process.Name -notin $targetNames) {
        continue
    }

    if ($process.CommandLine -match $rootPattern) {
        Add-TargetProcess -TargetIds $targetIds -ProcessId ([int]$process.ProcessId)
    }
}

$projectPorts = @($RustPort, $MysqlPort, $FrontendPort, $RedisPort, $JavaPort)
$portOwners = @(Get-NetTCPConnection -State Listen -LocalPort $projectPorts -ErrorAction SilentlyContinue)
$verifiedProjectPorts = @{}
foreach ($connection in $portOwners) {
    $ownerId = [int]$connection.OwningProcess
    $ownerPath = Get-ProcessPathSafe -ProcessId $ownerId
    if ($ownerPath -and ($ownerPath -match $rootPattern -or $ownerPath -match $envPattern)) {
        Add-TargetProcess -TargetIds $targetIds -ProcessId $ownerId
        $verifiedProjectPorts[[int]$connection.LocalPort] = $true
    } else {
        Write-Warning "Port $($connection.LocalPort) is in use by PID $ownerId outside the project; it was left untouched."
    }
}

$mysqlAdmin = Join-Path $EnvRoot "services\mysql\bin\mysqladmin.exe"
if ((Test-Path $mysqlAdmin) -and $verifiedProjectPorts.ContainsKey($MysqlPort)) {
    & $mysqlAdmin --protocol=tcp --host=127.0.0.1 --port=$MysqlPort --user=root --password= shutdown *> $null
}

$redisCli = Join-Path $EnvRoot "services\redis\redis-cli.exe"
if ((Test-Path $redisCli) -and $verifiedProjectPorts.ContainsKey($RedisPort)) {
    & $redisCli -p "$RedisPort" shutdown save *> $null
}

foreach ($processId in @($targetIds)) {
    Stop-TargetProcess -ProcessId $processId
}

Start-Sleep -Milliseconds 500
$remainingPorts = @(Get-NetTCPConnection -State Listen -LocalPort $projectPorts -ErrorAction SilentlyContinue)
foreach ($connection in $remainingPorts) {
    $ownerPath = Get-ProcessPathSafe -ProcessId ([int]$connection.OwningProcess)
    if ($ownerPath -and ($ownerPath -match $rootPattern -or $ownerPath -match $envPattern)) {
        Stop-TargetProcess -ProcessId ([int]$connection.OwningProcess)
    }
}

Write-Host "Project environment stopped."
