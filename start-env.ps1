[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path $PSScriptRoot).Path
$EnvRoot = Join-Path $Root "env"
$DataRoot = Join-Path $EnvRoot "data"
$LogsRoot = Join-Path $EnvRoot "logs"
$MysqlRoot = Join-Path $EnvRoot "services\mysql"
$RedisRoot = Join-Path $EnvRoot "services\redis"
$MysqlData = Join-Path $DataRoot "mysql"
$RedisData = Join-Path $DataRoot "redis"
$MysqlPort = 13306
$RedisPort = 16379
$RustPort = 13001
$JavaPort = 18086
$FrontendPort = 15173
$MavenCommand = Join-Path $EnvRoot "maven\apache-maven-3.9.9\bin\mvn.cmd"
$MavenRepository = Join-Path $EnvRoot "maven\repository"
$RuntimeConfig = Join-Path $EnvRoot "application-local.yml"

& (Join-Path $EnvRoot "setup.ps1")
if ($LASTEXITCODE -ne 0) {
    throw "Project environment setup failed."
}

New-Item -ItemType Directory -Force -Path $MysqlData, $RedisData, $LogsRoot | Out-Null

function Convert-ToConfigPath {
    param([string]$Path)
    $Path.Replace("\", "/")
}

function Test-TcpPort {
    param([int]$Port)
    $client = New-Object System.Net.Sockets.TcpClient
    try {
        $async = $client.BeginConnect("127.0.0.1", $Port, $null, $null)
        if (-not $async.AsyncWaitHandle.WaitOne(250)) {
            return $false
        }
        $client.EndConnect($async)
        return $true
    } catch {
        return $false
    } finally {
        $client.Dispose()
    }
}

function Invoke-NativeQuiet {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    & $FilePath @Arguments *> $null
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $oldErrorAction
    return $exitCode
}

$mysqlConfig = Join-Path $MysqlRoot "my.ini"
$mysqlRootConfig = Convert-ToConfigPath $MysqlRoot
$mysqlDataConfig = Convert-ToConfigPath $MysqlData
@"
[mysqld]
basedir=$mysqlRootConfig
datadir=$mysqlDataConfig
port=$MysqlPort
bind-address=127.0.0.1
character-set-server=utf8mb4
collation-server=utf8mb4_general_ci
max_connections=100

[client]
port=$MysqlPort
"@ | Set-Content -Encoding ascii -LiteralPath $mysqlConfig

$mysqlExe = Join-Path $MysqlRoot "bin\mysqld.exe"
$mysqlCli = Join-Path $MysqlRoot "bin\mysql.exe"
$mysqlAdmin = Join-Path $MysqlRoot "bin\mysqladmin.exe"
$mysqlLog = Join-Path $LogsRoot "mysql.log"
$mysqlErrorLog = Join-Path $LogsRoot "mysql-error.log"

if (-not (Test-Path (Join-Path $MysqlData "mysql"))) {
    Write-Host "Initializing project-local MySQL data..."
    & $mysqlExe "--defaults-file=$mysqlConfig" --initialize-insecure --console
    if ($LASTEXITCODE -ne 0) {
        throw "MySQL initialization failed."
    }
}

if (-not (Test-TcpPort $MysqlPort)) {
    Write-Host "Starting project-local MySQL on $MysqlPort..."
    $mysqlStartParams = @{
        FilePath = $mysqlExe
        ArgumentList = @("--defaults-file=$mysqlConfig", "--console")
        WorkingDirectory = $MysqlRoot
        RedirectStandardOutput = $mysqlLog
        RedirectStandardError = $mysqlErrorLog
        WindowStyle = "Hidden"
    }
    Start-Process @mysqlStartParams | Out-Null
}

$mysqlReady = $false
for ($i = 0; $i -lt 60; $i++) {
    $mysqlPingExit = Invoke-NativeQuiet -FilePath $mysqlAdmin -Arguments @(
        "--protocol=tcp",
        "--host=127.0.0.1",
        "--port=$MysqlPort",
        "--user=root",
        "--password=",
        "ping",
        "--silent"
    )
    if ($mysqlPingExit -eq 0) {
        $mysqlReady = $true
        break
    }
    Start-Sleep -Seconds 1
}
if (-not $mysqlReady) {
    throw "Project-local MySQL did not become ready. See $mysqlErrorLog."
}

$bootstrapSql = @"
CREATE DATABASE IF NOT EXISTS smart_tender_system CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci;
CREATE USER IF NOT EXISTS 'aibid'@'localhost' IDENTIFIED BY 'aibid-local';
CREATE USER IF NOT EXISTS 'aibid'@'127.0.0.1' IDENTIFIED BY 'aibid-local';
GRANT ALL PRIVILEGES ON smart_tender_system.* TO 'aibid'@'localhost';
GRANT ALL PRIVILEGES ON smart_tender_system.* TO 'aibid'@'127.0.0.1';
FLUSH PRIVILEGES;
"@
$oldErrorAction = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$bootstrapSql | & $mysqlCli "--protocol=tcp" "--host=127.0.0.1" "--port=$MysqlPort" "--user=root" "--password=" 2> $null
$bootstrapExit = $LASTEXITCODE
$ErrorActionPreference = $oldErrorAction
if ($bootstrapExit -ne 0) {
    throw "MySQL user/database initialization failed."
}

$schemaMarker = Join-Path $DataRoot ".schema-ready"
$schemaFile = Join-Path $Root "backend-java\src\main\resources\sql\smart_tender.sql"
if (-not (Test-Path $schemaMarker)) {
    Write-Host "Initializing project database schema..."
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $schemaCommand = "`"$mysqlCli`" --protocol=tcp --host=127.0.0.1 --port=$MysqlPort --user=root --password= --default-character-set=utf8mb4 < `"$schemaFile`""
    & cmd.exe /d /c $schemaCommand 2> $null
    $schemaExit = $LASTEXITCODE
    $ErrorActionPreference = $oldErrorAction
    if ($schemaExit -ne 0) {
        throw "Project database schema initialization failed."
    }
    New-Item -ItemType File -Force -Path $schemaMarker | Out-Null
}

$redisConfig = Join-Path $RedisRoot "ai-bid.conf"
$redisDataConfig = Convert-ToConfigPath $RedisData
$redisLog = Convert-ToConfigPath (Join-Path $LogsRoot "redis.log")
@"
bind 127.0.0.1
protected-mode yes
port $RedisPort
daemonize no
appendonly yes
dir "$redisDataConfig"
logfile "$redisLog"
"@ | Set-Content -Encoding ascii -LiteralPath $redisConfig

$redisExe = Join-Path $RedisRoot "redis-server.exe"
$redisCli = Join-Path $RedisRoot "redis-cli.exe"
if (-not (Test-TcpPort $RedisPort)) {
    Write-Host "Starting project-local Redis on $RedisPort..."
    $redisStartParams = @{
        FilePath = $redisExe
        ArgumentList = @($redisConfig)
        WorkingDirectory = $RedisRoot
        RedirectStandardOutput = (Join-Path $LogsRoot "redis-console.log")
        RedirectStandardError = (Join-Path $LogsRoot "redis-error.log")
        WindowStyle = "Hidden"
    }
    Start-Process @redisStartParams | Out-Null
}

$redisReady = $false
for ($i = 0; $i -lt 30; $i++) {
    $redisPingExit = Invoke-NativeQuiet -FilePath $redisCli -Arguments @("-p", "$RedisPort", "ping")
    if ($redisPingExit -eq 0) {
        $redisReady = $true
        break
    }
    Start-Sleep -Seconds 1
}
if (-not $redisReady) {
    throw "Project-local Redis did not become ready."
}

$envFile = Join-Path $Root ".env"
if (-not (Test-Path $envFile) -or -not (Select-String -Path $envFile -Pattern '^DASHSCOPE_API_KEY=\s*\S' -Quiet)) {
    Write-Warning "DASHSCOPE_API_KEY is empty in .env. AI requests will not work until it is filled."
}

$activate = Join-Path $EnvRoot "activate.bat"
$jdbcUrl = "jdbc:mysql://127.0.0.1:$MysqlPort/smart_tender_system?serverTimezone=Asia/Shanghai&useUnicode=true&characterEncoding=utf-8&zeroDateTimeBehavior=convertToNull&useSSL=false&allowPublicKeyRetrieval=true&socketTimeout=30000&connectTimeout=5000"
$jdbcCmdValue = $jdbcUrl.Replace("&", "^&")
$nodeCorepack = Join-Path $EnvRoot "node\corepack.cmd"

$storagePath = Convert-ToConfigPath (Join-Path $DataRoot "uploads")
$previewCachePath = Convert-ToConfigPath (Join-Path $DataRoot "preview-cache")
New-Item -ItemType Directory -Force -Path $storagePath, $previewCachePath | Out-Null
@"
server:
  address: 127.0.0.1
  port: $JavaPort

spring:
  datasource:
    url: '$jdbcUrl'
    username: aibid
    password: aibid-local
  data:
    redis:
      host: 127.0.0.1
      port: $RedisPort
      database: 10
  sql:
    init:
      mode: always
      schema-locations: classpath:audit_task_event.sql,classpath:trace_schema.sql

file:
  storage:
    path: $storagePath

preview:
  cache:
    path: $previewCachePath
  converter:
    base-url: http://127.0.0.1:18088

rust:
  api:
    base-url: http://127.0.0.1:$RustPort
"@ | Set-Content -Encoding utf8 -LiteralPath $RuntimeConfig
$runtimeConfigPath = (Resolve-Path $RuntimeConfig).Path.Replace("\", "/")

function Start-Console {
    param([Parameter(Mandatory = $true)][string]$Command)
    Start-Process -FilePath "cmd.exe" -WorkingDirectory $Root -WindowStyle Normal -ArgumentList @("/k", $Command) | Out-Null
}

function Start-ProjectConsole {
    param(
        [Parameter(Mandatory = $true)][int]$Port,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Command
    )

    if (Test-TcpPort $Port) {
        Write-Host "$Name is already listening on $Port."
        return
    }

    Start-Console -Command $Command
}

$rustCommand = "call `"$activate`" && cd /d `"$Root\backend-rust`" && set `"AIBID_RUST_BIND=127.0.0.1:$RustPort`" && cargo run --bin server"
$mavenRepoArgument = "-Dmaven.repo.local=$MavenRepository"
$javaRunArguments = "--server.address=127.0.0.1 --server.port=$JavaPort --spring.datasource.url=jdbc:mysql://127.0.0.1:$MysqlPort/smart_tender_system --spring.datasource.username=aibid --spring.datasource.password=aibid-local --spring.data.redis.host=127.0.0.1 --spring.data.redis.port=$RedisPort --spring.data.redis.database=10 --file.storage.path=$storagePath --preview.cache.path=$previewCachePath --preview.converter.base-url=http://127.0.0.1:18088 --rust.api.base-url=http://127.0.0.1:$RustPort"
$javaRunArgument = "-Dspring-boot.run.arguments=$javaRunArguments"
$javaCommand = "call `"$activate`" && set `"SPRING_CONFIG_ADDITIONAL_LOCATION=file:$runtimeConfigPath`" && cd /d `"$Root\backend-java`" && call `"$MavenCommand`" `"$mavenRepoArgument`" `"$javaRunArgument`" spring-boot:run"
$frontendCommand = "call `"$activate`" && set `"AIBID_JAVA_BASE_URL=http://127.0.0.1:$JavaPort`" && set `"AIBID_FRONTEND_PORT=$FrontendPort`" && cd /d `"$Root\frontend`" && call `"$nodeCorepack`" pnpm install --frozen-lockfile && call `"$nodeCorepack`" pnpm dev"

Start-ProjectConsole -Port $RustPort -Name "Rust API" -Command $rustCommand
Start-Sleep -Seconds 3
Start-ProjectConsole -Port $JavaPort -Name "Java API" -Command $javaCommand
Start-ProjectConsole -Port $FrontendPort -Name "Frontend" -Command $frontendCommand

Write-Host ""
Write-Host "Project environment started without Docker or WSL."
Write-Host "Frontend: http://127.0.0.1:$FrontendPort"
Write-Host "Java API: http://127.0.0.1:$JavaPort"
Write-Host "Rust API: http://127.0.0.1:$RustPort"
Start-Process "http://127.0.0.1:$FrontendPort" | Out-Null
