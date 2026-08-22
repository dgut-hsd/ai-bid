package com.ithsd.smart_tender.service.impl;

import com.baomidou.mybatisplus.core.conditions.query.LambdaQueryWrapper;
import com.ithsd.smart_tender.mapper.KnowledgeChunkMapper;
import com.ithsd.smart_tender.model.entity.KnowledgeChunk;
import com.ithsd.smart_tender.service.KnowledgeChunkService;
import lombok.extern.slf4j.Slf4j;
import lombok.RequiredArgsConstructor;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.io.IOException;
import java.util.Collections;
import java.util.List;

@Slf4j
@Service
@RequiredArgsConstructor
public class KnowledgeChunkServiceImpl implements KnowledgeChunkService {

    private final KnowledgeChunkMapper chunkMapper;

    @Override
    @Transactional(rollbackFor = Exception.class)
    public void processFileChunks(Long fileId, String filePath, String namespace) throws IOException {
        TenantScope.requiredTenantId();
        log.info("Java端切片已关闭，跳过：fileId={}, filePath={}", fileId, filePath);
    }

    @Override
    public List<KnowledgeChunk> getChunksByFileId(Long fileId) {
        Long tenantId = TenantScope.requiredTenantId();
        return chunkMapper.selectList(new LambdaQueryWrapper<KnowledgeChunk>()
                .eq(KnowledgeChunk::getFileId, fileId)
                .eq(KnowledgeChunk::getTenantId, tenantId)
                .orderByAsc(KnowledgeChunk::getChunkIndex));
    }

    @Override
    @Transactional(rollbackFor = Exception.class)
    public void deleteChunksByFileId(Long fileId) {
        Long tenantId = TenantScope.requiredTenantId();
        chunkMapper.delete(new LambdaQueryWrapper<KnowledgeChunk>()
                .eq(KnowledgeChunk::getFileId, fileId)
                .eq(KnowledgeChunk::getTenantId, tenantId));
        log.info("Java端切片已关闭，跳过删除：fileId={}", fileId);
    }
}
