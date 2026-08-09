import React from 'react';
import {
  Table,
  Button,
  Modal,
  Form,
  Input,
  Tag,
  Space,
  App,
  Card,
  Typography,
} from 'antd';
import { PlusOutlined, TeamOutlined, SwapOutlined } from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import { useTenant } from '../hooks/useTenant';
import { tenantApi } from '../api/tenant';
import { useQuery, useMutation } from '@tanstack/react-query';
import type { TenantSummary, TenantMember, CreateTenantParams } from '../types';

const { Title } = Typography;

export const TenantManagePage: React.FC = () => {
  const { message } = App.useApp();
  const [createForm] = Form.useForm<CreateTenantParams>();
  const [createModalOpen, setCreateModalOpen] = React.useState(false);
  const [memberModalOpen, setMemberModalOpen] = React.useState(false);
  const [selectedTenant, setSelectedTenant] = React.useState<TenantSummary | null>(null);
  const [memberPage, setMemberPage] = React.useState(1);

  const {
    tenantList,
    currentTenantId,
    isLoading,
    isSwitching,
    switchTenant,
    refetchTenants,
  } = useTenant();

  // ── 创建租户 ──────────────────────────────────────────────────────
  const createMutation = useMutation({
    mutationFn: (params: CreateTenantParams) => tenantApi.createTenant(params),
    onSuccess: (resp) => {
      if (resp.code === 200) {
        message.success('租户创建成功');
        setCreateModalOpen(false);
        createForm.resetFields();
        refetchTenants();
      } else {
        message.error(resp.msg || '创建失败');
      }
    },
    onError: (error: any) => {
      message.error(error?.response?.data?.msg || '创建失败');
    },
  });

  // ── 成员列表 ──────────────────────────────────────────────────────
  const membersQuery = useQuery({
    queryKey: ['tenant-members', selectedTenant?.tenant_id, memberPage],
    queryFn: async () => {
      if (!selectedTenant) return null;
      const resp = await tenantApi.getMembers(
        selectedTenant.tenant_id,
        memberPage,
        20
      );
      if (resp.code === 200) return resp.data;
      throw new Error(resp.msg);
    },
    enabled: !!selectedTenant && memberModalOpen,
  });

  // ── 租户列表表格列 ────────────────────────────────────────────────
  const columns: ColumnsType<TenantSummary> = [
    {
      title: '租户名称',
      dataIndex: 'name',
      key: 'name',
      render: (text: string, record: TenantSummary) => (
        <Space>
          <span style={{ fontWeight: record.tenant_id === currentTenantId ? 600 : 400 }}>
            {text}
          </span>
          {record.tenant_id === currentTenantId && (
            <Tag color='green' style={{ margin: 0 }}>当前</Tag>
          )}
        </Space>
      ),
    },
    {
      title: '角色',
      dataIndex: 'role',
      key: 'role',
      render: (role?: string) => {
        if (!role) return '-';
        const color = role === 'owner' ? 'gold' : 'blue';
        const label = role === 'owner' ? '拥有者' : role === 'admin' ? '管理员' : '成员';
        return <Tag color={color}>{label}</Tag>;
      },
    },
    {
      title: '创建时间',
      dataIndex: 'created_at',
      key: 'created_at',
      render: (t?: string) => (t ? new Date(t).toLocaleString('zh-CN') : '-'),
    },
    {
      title: '操作',
      key: 'action',
      width: 200,
      render: (_: unknown, record: TenantSummary) => (
        <Space>
          <Button
            size='small'
            type='link'
            icon={<TeamOutlined />}
            disabled={record.tenant_id !== currentTenantId && record.role !== 'owner' && record.role !== 'admin'}
            onClick={() => {
              setSelectedTenant(record);
              setMemberPage(1);
              setMemberModalOpen(true);
            }}
          >
            成员
          </Button>
          {record.tenant_id !== currentTenantId && (
            <Button
              size='small'
              type='link'
              icon={<SwapOutlined />}
              loading={isSwitching}
              onClick={() => switchTenant(record.tenant_id)}
            >
              切换
            </Button>
          )}
        </Space>
      ),
    },
  ];

  // ── 成员列表表格列 ────────────────────────────────────────────────
  const memberColumns: ColumnsType<TenantMember> = [
    {
      title: '用户名',
      dataIndex: 'username',
      key: 'username',
    },
    {
      title: '姓名',
      dataIndex: 'real_name',
      key: 'real_name',
      render: (t?: string) => t || '-',
    },
    {
      title: '角色',
      dataIndex: 'role',
      key: 'role',
      render: (role: string) => {
        const color = role === 'owner' ? 'gold' : role === 'admin' ? 'blue' : 'default';
        const label = role === 'owner' ? '拥有者' : role === 'admin' ? '管理员' : '成员';
        return <Tag color={color}>{label}</Tag>;
      },
    },
    {
      title: '加入时间',
      dataIndex: 'joined_at',
      key: 'joined_at',
      render: (t?: string) => (t ? new Date(t).toLocaleString('zh-CN') : '-'),
    },
  ];

  return (
    <div style={{ padding: 24 }}>
      <Card
        title={
          <Space>
            <Title level={4} style={{ margin: 0 }}>租户管理</Title>
          </Space>
        }
        extra={
          <Button
            type='primary'
            icon={<PlusOutlined />}
            onClick={() => setCreateModalOpen(true)}
          >
            创建租户
          </Button>
        }
      >
        <Table
          rowKey='tenant_id'
          columns={columns}
          dataSource={tenantList}
          loading={isLoading}
          pagination={false}
          size='middle'
        />
      </Card>

      {/* ── 创建租户弹窗 ─────────────────────────────────────────── */}
      <Modal
        title='创建租户'
        open={createModalOpen}
        onCancel={() => {
          setCreateModalOpen(false);
          createForm.resetFields();
        }}
        onOk={() => createForm.submit()}
        confirmLoading={createMutation.isPending}
        okText='创建'
        cancelText='取消'
      >
        <Form
          form={createForm}
          layout='vertical'
          onFinish={(values) => createMutation.mutate(values)}
        >
          <Form.Item
            name='name'
            label='租户名称'
            rules={[
              { required: true, message: '请输入租户名称' },
              { max: 50, message: '名称不超过 50 个字符' },
            ]}
          >
            <Input placeholder='请输入租户名称' />
          </Form.Item>
          <Form.Item
            name='description'
            label='描述（选填）'
            rules={[{ max: 200, message: '描述不超过 200 个字符' }]}
          >
            <Input.TextArea rows={3} placeholder='选填' />
          </Form.Item>
        </Form>
      </Modal>

      {/* ── 成员列表弹窗 ─────────────────────────────────────────── */}
      <Modal
        title={`成员列表 — ${selectedTenant?.name || ''}`}
        open={memberModalOpen}
        onCancel={() => {
          setMemberModalOpen(false);
          setSelectedTenant(null);
        }}
        footer={null}
        width={640}
      >
        <Table
          rowKey='user_id'
          columns={memberColumns}
          dataSource={membersQuery.data?.items || []}
          loading={membersQuery.isLoading}
          size='middle'
          pagination={{
            current: memberPage,
            pageSize: 20,
            total: membersQuery.data?.total || 0,
            onChange: (page) => setMemberPage(page),
            showTotal: (total) => `共 ${total} 名成员`,
            size: 'default',
          }}
        />
      </Modal>
    </div>
  );
};
