# ── Java 后端多阶段构建：Maven 构建 → JRE 运行 ──
FROM maven:3.9-eclipse-temurin-17 AS build

WORKDIR /app

# 先复制 pom，利用依赖缓存（pom 已配置阿里云 Maven 镜像，加速下载）
COPY pom.xml ./
RUN mvn -B dependency:go-offline

# 复制源码并打包（跳过测试；Flyway 迁移文件会打进 jar）
COPY src ./src
RUN mvn -B clean package -DskipTests

# ── 运行阶段 ──
FROM eclipse-temurin:17-jre

WORKDIR /app

# 拷贝可执行 fat jar（名称 = artifactId-version.jar）
COPY --from=build /app/target/smart_tender-0.0.1-SNAPSHOT.jar app.jar

# 数据目录（上传文件、预览缓存），宿主机挂卷
VOLUME /data

EXPOSE 3000

ENTRYPOINT ["java", "-jar", "app.jar"]