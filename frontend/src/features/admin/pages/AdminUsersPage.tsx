import React from 'react';
import {
  Table,
  Button,
  Modal,
  Form,
  Input,
  Select,
  Tag,
  Space,
  App,
  Card,
  Typography,
  Dropdown,
} from 'antd';
import { PlusOutlined, KeyOutlined, UserDeleteOutlined, EditOutlined, MoreOutlined } from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useIsMobile } from '@/hooks/useMediaQuery';
import { adminApi } from '../api/admin';
import type { AdminUser, AdminRole, CreateUserParams } from '../types';

const { Title } = Typography;

const ROLE_LABEL: Record<AdminRole, { text: string; color: string }> = {
  OWNER: { text: '拥有者', color: 'gold' },
  MEMBER: { text: '成员', color: 'default' },
};

export const AdminUsersPage: React.FC = () => {
  const { message, modal } = App.useApp();
  const queryClient = useQueryClient();

  const [createModalOpen, setCreateModalOpen] = React.useState(false);
  const [resetTarget, setResetTarget] = React.useState<AdminUser | null>(null);
  const [editTarget, setEditTarget] = React.useState<AdminUser | null>(null);
  const [createForm] = Form.useForm<CreateUserParams>();
  const [resetForm] = Form.useForm<{ password: string }>();
  const [editForm] = Form.useForm<{ username: string; real_name: string }>();
  const isMobile = useIsMobile();

  const usersQuery = useQuery({
    queryKey: ['admin-users'],
    queryFn: async () => {
      const resp = await adminApi.listUsers();
      if (resp.code === 200 && resp.data) return resp.data;
      throw new Error(resp.msg || '获取用户列表失败');
    },
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ['admin-users'] });

  const createMutation = useMutation({
    mutationFn: (data: CreateUserParams) => adminApi.createUser(data),
    onSuccess: (resp) => {
      if (resp.code === 200) {
        message.success('用户创建成功');
        setCreateModalOpen(false);
        createForm.resetFields();
        invalidate();
      } else {
        message.error(resp.msg || '创建失败');
      }
    },
    onError: (error: any) => {
      message.error(error?.response?.data?.msg || '创建失败');
    },
  });

  const resetMutation = useMutation({
    mutationFn: ({ userId, password }: { userId: string; password: string }) =>
      adminApi.resetPassword(userId, password),
    onSuccess: (resp) => {
      if (resp.code === 200) {
        message.success('密码已重置');
        setResetTarget(null);
        resetForm.resetFields();
      } else {
        message.error(resp.msg || '重置失败');
      }
    },
    onError: (error: any) => {
      message.error(error?.response?.data?.msg || '重置失败');
    },
  });

  const removeMutation = useMutation({
    mutationFn: (userId: string) => adminApi.removeMember(userId),
    onSuccess: (resp) => {
      if (resp.code === 200) {
        message.success('成员已移除');
        invalidate();
      } else {
        message.error(resp.msg || '移除失败');
      }
    },
    onError: (error: any) => {
      message.error(error?.response?.data?.msg || error?.response?.data?.message || '移除失败');
    },
  });

  const updateMutation = useMutation({
    mutationFn: ({
      userId,
      username,
      real_name,
    }: {
      userId: string;
      username: string;
      real_name: string;
    }) => adminApi.updateUser(userId, { username, real_name }),
    onSuccess: (resp) => {
      if (resp.code === 200) {
        message.success('姓名已更新');
        setEditTarget(null);
        editForm.resetFields();
        invalidate();
      } else {
        message.error(resp.msg || '更新失败');
      }
    },
    onError: (error: any) => {
      message.error(error?.response?.data?.msg || '更新失败');
    },
  });

  const columns: ColumnsType<AdminUser> = [
    { title: '账号', dataIndex: 'username', key: 'username' },
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
      render: (role: AdminRole) => {
        const item = ROLE_LABEL[role] ?? { text: role, color: 'default' };
        return <Tag color={item.color}>{item.text}</Tag>;
      },
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      render: (status?: string) =>
        status === 'ACTIVE' ? <Tag color='green'>启用</Tag> : <Tag color='red'>停用</Tag>,
    },
    {
      title: '创建时间',
      dataIndex: 'created_at',
      key: 'created_at',
      responsive: ['md', 'lg', 'xl', 'xxl'],
      render: (t?: string) => (t ? new Date(t).toLocaleString('zh-CN') : '-'),
    },
    {
      title: '操作',
      key: 'action',
      width: isMobile ? 76 : 180,
      fixed: 'right',
      render: (_: unknown, record: AdminUser) => {
        const editAction = () => {
          setEditTarget(record);
          editForm.setFieldsValue({
            username: record.username,
            real_name: record.real_name ?? '',
          });
        };
        const resetAction = () => {
          setResetTarget(record);
          resetForm.resetFields();
        };
        const removeAction = () => {
          modal.confirm({
            title: `确定移除「${record.real_name || record.username}」吗？`,
            content: '移除后该账号将被停用，无法再登录。',
            okText: '移除',
            cancelText: '取消',
            okButtonProps: { danger: true },
            onOk: () => removeMutation.mutate(record.user_id),
          });
        };

        // 手机端：三个操作合并进「更多」下拉，点击展开
        if (isMobile) {
          return (
            <Dropdown
              trigger={['click']}
              menu={{
                items: [
                  { key: 'edit', icon: <EditOutlined />, label: '编辑', onClick: editAction },
                  { key: 'reset', icon: <KeyOutlined />, label: '重置密码', onClick: resetAction },
                  {
                    key: 'remove',
                    icon: <UserDeleteOutlined />,
                    danger: true,
                    label: '移除',
                    onClick: removeAction,
                  },
                ],
              }}
            >
              <Button size='small' aria-label='更多操作' icon={<MoreOutlined />} />
            </Dropdown>
          );
        }

        return (
          <Space>
            <Button size='small' type='link' icon={<EditOutlined />} onClick={editAction}>
              编辑
            </Button>
            <Button size='small' type='link' icon={<KeyOutlined />} onClick={resetAction}>
              重置密码
            </Button>
            <Button size='small' type='link' danger icon={<UserDeleteOutlined />} onClick={removeAction}>
              移除
            </Button>
          </Space>
        );
      },
    },
  ];

  return (
    <div style={{ padding: 24 }}>
      <Card
        title={
          <Space>
            <Title level={4} style={{ margin: 0 }}>用户管理</Title>
          </Space>
        }
        extra={
          <Button
            type='primary'
            icon={<PlusOutlined />}
            onClick={() => {
              createForm.resetFields();
              setCreateModalOpen(true);
            }}
          >
            创建用户
          </Button>
        }
      >
        <Table
          rowKey='user_id'
          columns={columns}
          dataSource={usersQuery.data || []}
          loading={usersQuery.isLoading}
          pagination={false}
          size='middle'
          scroll={{ x: 'max-content' }}
        />
      </Card>

      {/* ── 创建用户弹窗 ─────────────────────────────────────────── */}
      <Modal
        title='创建用户'
        open={createModalOpen}
        onCancel={() => setCreateModalOpen(false)}
        onOk={() => createForm.submit()}
        confirmLoading={createMutation.isPending}
        okText='创建'
        cancelText='取消'
      >
        <Form
          form={createForm}
          layout='vertical'
          initialValues={{ role: 'MEMBER' }}
          onFinish={(values) => createMutation.mutate(values)}
        >
          <Form.Item
            name='username'
            label='账号'
            rules={[
              { required: true, message: '请输入账号' },
              { min: 3, max: 50, message: '账号长度 3~50 个字符' },
            ]}
          >
            <Input placeholder='用于登录的用户名' autoComplete='off' />
          </Form.Item>
          <Form.Item
            name='password'
            label='初始密码'
            rules={[
              { required: true, message: '请输入初始密码' },
              { min: 6, max: 100, message: '密码长度 6~100 个字符' },
            ]}
          >
            <Input.Password placeholder='至少 6 位' autoComplete='new-password' />
          </Form.Item>
          <Form.Item
            name='real_name'
            label='姓名'
            rules={[{ required: true, message: '请输入姓名' }]}
          >
            <Input placeholder='真实姓名' />
          </Form.Item>
          <Form.Item name='role' label='角色' rules={[{ required: true }]}>
            <Select
              options={[
                { value: 'OWNER', label: '拥有者（可管理用户）' },
                { value: 'MEMBER', label: '成员（普通业务用户）' },
              ]}
            />
          </Form.Item>
        </Form>
      </Modal>

      {/* ── 编辑用户（姓名）弹窗 ─────────────────────────────────── */}
      <Modal
        title={`编辑用户 — ${editTarget?.username || ''}`}
        open={!!editTarget}
        onCancel={() => setEditTarget(null)}
        onOk={() => editForm.submit()}
        confirmLoading={updateMutation.isPending}
        okText='保存'
        cancelText='取消'
      >
        <Form
          form={editForm}
          layout='vertical'
          onFinish={(values) =>
            editTarget &&
            updateMutation.mutate({
              userId: editTarget.user_id,
              username: values.username,
              real_name: values.real_name,
            })
          }
        >
          <Form.Item
            name='username'
            label='账号'
            rules={[
              { required: true, message: '请输入账号' },
              { min: 3, max: 50, message: '账号长度 3~50 个字符' },
            ]}
          >
            <Input placeholder='登录账号' autoComplete='off' />
          </Form.Item>
          <Form.Item
            name='real_name'
            label='姓名'
            rules={[{ required: true, message: '请输入姓名' }]}
          >
            <Input placeholder='真实姓名' />
          </Form.Item>
        </Form>
      </Modal>

      {/* ── 重置密码弹窗 ─────────────────────────────────────────── */}
      <Modal
        title={`重置密码 — ${resetTarget?.real_name || resetTarget?.username || ''}`}
        open={!!resetTarget}
        onCancel={() => setResetTarget(null)}
        onOk={() => resetForm.submit()}
        confirmLoading={resetMutation.isPending}
        okText='确定'
        cancelText='取消'
      >
        <Form
          form={resetForm}
          layout='vertical'
          onFinish={(values) =>
            resetTarget && resetMutation.mutate({ userId: resetTarget.user_id, password: values.password })
          }
        >
          <Form.Item
            name='password'
            label='新密码'
            rules={[
              { required: true, message: '请输入新密码' },
              { min: 6, max: 100, message: '密码长度 6~100 个字符' },
            ]}
          >
            <Input.Password placeholder='新密码' autoComplete='new-password' />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
};