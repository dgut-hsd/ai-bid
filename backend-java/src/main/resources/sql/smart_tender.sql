CREATE DATABASE IF NOT EXISTS `smart_tender_system` DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci;
USE `smart_tender_system`;

DROP TABLE IF EXISTS `sys_user`;
CREATE TABLE `sys_user` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT COMMENT '用户ID',
  `username` varchar(50) NOT NULL COMMENT '用户名',
  `password` varchar(200) NOT NULL COMMENT '加密密码',
  `real_name` varchar(50) DEFAULT NULL COMMENT '真实姓名',
  `email` varchar(100) DEFAULT NULL COMMENT '邮箱',
  `phone` varchar(20) DEFAULT NULL COMMENT '电话',
  `status` tinyint(4) DEFAULT 1 COMMENT '状态（0停用 1启用）',
  `create_time` datetime DEFAULT NULL COMMENT '创建时间',
  `update_time` datetime DEFAULT NULL COMMENT '更新时间',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_username` (`username`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='用户表';

DROP TABLE IF EXISTS `bid_document`;
CREATE TABLE `bid_document` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT COMMENT '文件ID',
  `file_name` varchar(200) NOT NULL COMMENT '文件名',
  `file_path` varchar(500) NOT NULL COMMENT '存储路径',
  `file_size` bigint(20) DEFAULT NULL COMMENT '文件大小（字节）',
  `file_type` varchar(20) DEFAULT NULL COMMENT '文件类型（word/pdf）',
  `file_category` varchar(20) DEFAULT NULL COMMENT '文件作用类型（bid/contract）',
  `bid_name` varchar(200) DEFAULT NULL COMMENT '项目名称',
  `supplier_name` varchar(200) DEFAULT NULL COMMENT '供应商名称',
  `budget_amount` decimal(15,2) DEFAULT NULL COMMENT '预算金额',
  `page_count` int(11) DEFAULT NULL COMMENT '页数',
  `parse_status` tinyint(4) DEFAULT 0 COMMENT '解析状态（0待解析 1解析中 2已完成 3失败）',
  `upload_user_id` bigint(20) DEFAULT NULL COMMENT '上传用户ID',
  `upload_time` datetime DEFAULT NULL COMMENT '上传时间',
  `version` int(11) DEFAULT 1 COMMENT '版本号',
  `project_id` bigint(20) DEFAULT NULL COMMENT '项目ID',
  `rust_document_id` varchar(64) DEFAULT NULL COMMENT 'Rust 审核引擎文档ID',
  PRIMARY KEY (`id`),
  KEY `idx_upload_user_id` (`upload_user_id`),
  KEY `idx_file_category` (`file_category`),
  KEY `idx_upload_time` (`upload_time`),
  KEY `idx_project_id` (`project_id`),
  KEY `idx_rust_document_id` (`rust_document_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='标书文件表';

DROP TABLE IF EXISTS `audit_task`;
CREATE TABLE `audit_task` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT COMMENT '任务ID',
  `task_id` varchar(64) NOT NULL COMMENT '任务唯一标识',
  `bid_id` bigint(20) NOT NULL COMMENT '标书ID',
  `task_status` tinyint(4) DEFAULT 0 COMMENT '任务状态（0待处理 1审核中 2已完成 3失败）',
  `audit_result` varchar(20) DEFAULT NULL COMMENT '审核结果（pass通过/reject不通过/revise需修改）',
  `issue_count` int(11) DEFAULT 0 COMMENT '问题数量',
  `critical_count` int(11) DEFAULT 0 COMMENT '严重问题数',
  `warning_count` int(11) DEFAULT 0 COMMENT '一般问题数',
  `info_count` int(11) DEFAULT 0 COMMENT '提示信息数',
  `start_time` datetime DEFAULT NULL COMMENT '开始时间',
  `end_time` datetime DEFAULT NULL COMMENT '结束时间',
  `audit_user_id` bigint(20) DEFAULT NULL COMMENT '审核人ID',
  `create_time` datetime DEFAULT NULL COMMENT '创建时间',
  `stage` varchar(64) DEFAULT NULL COMMENT '当前审核阶段',
  `progress` int(11) NOT NULL DEFAULT 0 COMMENT '审核进度（0-100）',
  `enabled_checks` json DEFAULT NULL COMMENT '启用的审核项',
  `failed_stages` json DEFAULT NULL COMMENT '失败的审核阶段',
  `error_msg` varchar(1000) DEFAULT NULL COMMENT '失败原因',
  `updated_at` datetime DEFAULT NULL COMMENT '更新时间',
  `version` bigint(20) NOT NULL DEFAULT 0 COMMENT '乐观锁版本号',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_task_id` (`task_id`),
  KEY `idx_bid_id` (`bid_id`),
  KEY `idx_task_status` (`task_status`),
  KEY `idx_audit_user_id` (`audit_user_id`),
  KEY `idx_create_time` (`create_time`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='审核任务表';

DROP TABLE IF EXISTS `audit_issue`;
CREATE TABLE `audit_issue` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT COMMENT '问题ID',
  `audit_id` bigint(20) NOT NULL COMMENT '审核任务ID',
  `issue_no` varchar(20) DEFAULT NULL COMMENT '问题编号',
  `severity` varchar(20) DEFAULT NULL COMMENT '四级严重程度（high/medium/low/info）',
  `is_critical` tinyint(1) NOT NULL DEFAULT 0 COMMENT '是否重大/红线问题',
  `critical_reason` text COMMENT '重大问题判定依据',
  `category` varchar(50) DEFAULT NULL COMMENT '风险类型（Rust引擎risk_type，如：地域歧视/品牌指定/程序违规）',
  `description` text COMMENT '问题描述',
  `suggestion` text COMMENT '修改建议',
  `page_number` int(11) DEFAULT NULL COMMENT '页码',
  `section_name` varchar(200) DEFAULT NULL COMMENT '章节名',
  `context` text COMMENT '上下文片段',
  `reference` text COMMENT '标准依据',
  `create_time` datetime DEFAULT NULL COMMENT '创建时间',
  PRIMARY KEY (`id`),
  KEY `idx_audit_id` (`audit_id`),
  KEY `idx_severity` (`severity`),
  KEY `idx_audit_issue_critical` (`is_critical`),
  KEY `idx_category` (`category`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='审核问题表';

DROP TABLE IF EXISTS `audit_report`;
CREATE TABLE `audit_report` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT COMMENT '报告ID',
  `audit_id` bigint(20) NOT NULL COMMENT '审核任务ID',
  `doc_content` TEXT NOT NULL COMMENT 'Markdown文档内容（64KB上限，适配绝大多数场景）',
  `version` int(11) DEFAULT 1 COMMENT '版本号',
/*   `report_path` varchar(500) NOT NULL COMMENT '报告文件路径',
   `file_size` bigint(20) DEFAULT NULL COMMENT '文件大小', */
  `generate_time` datetime DEFAULT NULL COMMENT '生成时间',
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_audit_id` (`audit_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='审核报告表';

DROP TABLE IF EXISTS `knowledge_file`;
CREATE TABLE `knowledge_file` (

  `id` bigint(20) NOT NULL AUTO_INCREMENT COMMENT '文件ID',
  `file_name` varchar(200) NOT NULL COMMENT '文件名',
  `file_path` varchar(500) NOT NULL COMMENT '存储路径',
  `file_size` bigint(20) DEFAULT NULL COMMENT '文件大小',
  `file_type` varchar(50) DEFAULT NULL COMMENT '文件类型',
  `category` varchar(50) DEFAULT NULL COMMENT '分类',
  `applicable_scope` varchar(50) DEFAULT NULL COMMENT '适用范围(procurement/engineering/general)',
  `tags` varchar(200) DEFAULT NULL COMMENT '标签',
  `description` text COMMENT '用途描述',
  `status` tinyint(4) DEFAULT 1 COMMENT '状态（0停用 1启用 2已删除）',
  `version` int(11) DEFAULT 1 COMMENT '版本号',
  `chunk_count` int(11) DEFAULT 0 COMMENT '分块数量',
  `rust_document_id` varchar(64) DEFAULT NULL COMMENT 'Rust/Qdrant 文档ID（向量化入库成功后回填，删除时用于清理向量）',
  `upload_user_id` bigint(20) DEFAULT NULL COMMENT '上传用户ID',
  `upload_time` datetime DEFAULT NULL COMMENT '上传时间',
  `update_time` datetime DEFAULT NULL COMMENT '更新时间',
  PRIMARY KEY (`id`),
  KEY `idx_category` (`category`),
  KEY `idx_status` (`status`),
  KEY `idx_upload_user_id` (`upload_user_id`),
  KEY `idx_upload_time` (`upload_time`),
  KEY `idx_rust_document_id` (`rust_document_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='标准库文件表';

DROP TABLE IF EXISTS `knowledge_chunk`;
CREATE TABLE `knowledge_chunk` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT COMMENT '分块ID',
  `file_id` bigint(20) NOT NULL COMMENT '文件ID',
  `chunk_index` int(11) NOT NULL COMMENT '块序号',
  `chunk_text` text COMMENT '文本内容',
  `chunk_length` int(11) DEFAULT NULL COMMENT '文本长度',
  `vector_id` varchar(100) DEFAULT NULL COMMENT 'Qdrant中的向量ID',
  `page_number` int(11) DEFAULT NULL COMMENT '所在页码',
  `section_name` varchar(200) DEFAULT NULL COMMENT '所在章节',
  `create_time` datetime DEFAULT NULL COMMENT '创建时间',
  PRIMARY KEY (`id`),
  KEY `idx_file_id` (`file_id`),
  KEY `idx_vector_id` (`vector_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='知识库分块表';

DROP TABLE IF EXISTS `chat_message`;
CREATE TABLE `chat_message` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT COMMENT '消息ID',
  `project_id` bigint(20) NOT NULL COMMENT '项目ID',
  `bid_id` bigint(20) NOT NULL COMMENT '标书ID',
  `user_id` bigint(20) NOT NULL COMMENT '用户ID',
  `role` varchar(20) NOT NULL COMMENT '角色（user/assistant）',
  `content` text NOT NULL COMMENT '消息内容',
  `create_time` datetime DEFAULT NULL COMMENT '创建时间',
  PRIMARY KEY (`id`),
  KEY `idx_project_id` (`project_id`),
  KEY `idx_bid_id` (`bid_id`),
  KEY `idx_user_id` (`user_id`),
  KEY `idx_create_time` (`create_time`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='对话消息表';

DROP TABLE IF EXISTS `project`;
CREATE TABLE `project` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT COMMENT '项目ID',
  `user_id` bigint(20) NOT NULL COMMENT '用户ID',
  `project_name` varchar(200) NOT NULL COMMENT '项目名称',
  `supplier_name` varchar(200) DEFAULT NULL COMMENT '供应商名称',
  `parse_status` tinyint(4) DEFAULT 0 COMMENT '审核状态（0未审核 1已审核）',
  `latest_version` int(11) DEFAULT 1 COMMENT '最新版本号',
  `create_time` datetime DEFAULT NULL COMMENT '创建时间',
  `update_time` datetime DEFAULT NULL COMMENT '更新时间',
  PRIMARY KEY (`id`),
  KEY `idx_user_id` (`user_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='项目表';
